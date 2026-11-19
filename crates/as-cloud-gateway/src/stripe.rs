//! Stripe: a hosted payment page for each top-up, and the events that say
//! it was paid, refunded or disputed. Cards and PayPal both come through
//! Checkout (PayPal is switched on in the Stripe Dashboard, not here).

use serde_json::Value;

use crate::config;

/// Making a payment page. A trait so tests need no Stripe.
pub trait Payments: Send + Sync {
    /// Stripe Checkout's address for a top-up of `cents` euro cents to `account`.
    fn checkout(&self, account: i64, support_id: &str, cents: i64) -> Result<String, String>;
}

pub struct StripeApi {
    pub cfg: config::Stripe,
}

impl Payments for StripeApi {
    fn checkout(&self, account: i64, support_id: &str, cents: i64) -> Result<String, String> {
        if self.cfg.secret_key.is_empty() {
            return Err("no Stripe key configured".into());
        }
        let account = account.to_string();
        let cents_s = cents.to_string();
        let name = format!("AutoScene Cloud balance: {}", as_cloud::euros(cents * as_cloud::MICROS_PER_CENT));
        let mut form: Vec<(&str, &str)> = vec![
            ("mode", "payment"),
            ("line_items[0][quantity]", "1"),
            ("line_items[0][price_data][currency]", "eur"),
            ("line_items[0][price_data][unit_amount]", &cents_s),
            ("line_items[0][price_data][product_data][name]", &name),
            ("client_reference_id", &account),
            ("metadata[account]", &account),
            ("metadata[support_id]", support_id),
            ("metadata[cents]", &cents_s),
            ("payment_intent_data[metadata][account]", &account),
            ("payment_intent_data[metadata][support_id]", support_id),
            ("success_url", &self.cfg.success_url),
            ("cancel_url", &self.cfg.cancel_url),
        ];
        if let Some(footer) = &self.cfg.invoice_footer {
            form.push(("invoice_creation[enabled]", "true"));
            form.push(("invoice_creation[invoice_data][footer]", footer));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        let mut resp = agent
            .post("https://api.stripe.com/v1/checkout/sessions")
            .header("Authorization", &format!("Bearer {}", self.cfg.secret_key))
            .header("Idempotency-Key", &crate::crypto::token())
            .send_form(form)
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        let raw = resp.body_mut().read_to_vec().map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("Stripe answered {status}: {}", body["error"]["message"]));
        }
        body["url"].as_str().map(str::to_string).ok_or_else(|| "Stripe sent no url".into())
    }
}

/// What a Stripe event means for the ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A checkout was paid: `cents` go on `account`.
    Credit {
        checkout: String,
        account: i64,
        cents: i64,
        payment_intent: Option<String>,
    },
    /// So much of a payment has been refunded in all.
    Refunded {
        payment_intent: String,
        total_cents: i64,
    },
    Disputed {
        payment_intent: String,
    },
    Ignore,
}

/// Read an event Stripe signed. Euros only: a payment in anything else is
/// not one of ours.
pub fn action(event: &Value) -> Action {
    let obj = &event["data"]["object"];
    match event["type"].as_str().unwrap_or("") {
        "checkout.session.completed" | "checkout.session.async_payment_succeeded" => {
            if obj["payment_status"].as_str() != Some("paid") || obj["currency"].as_str() != Some("eur") {
                return Action::Ignore;
            }
            let account = obj["client_reference_id"]
                .as_str()
                .or(obj["metadata"]["account"].as_str())
                .and_then(|s| s.parse::<i64>().ok());
            match (obj["id"].as_str(), account, obj["amount_total"].as_i64()) {
                (Some(id), Some(account), Some(cents)) if cents > 0 => Action::Credit {
                    checkout: id.to_string(),
                    account,
                    cents,
                    payment_intent: obj["payment_intent"].as_str().map(str::to_string),
                },
                _ => Action::Ignore,
            }
        }
        "charge.refunded" => match (obj["payment_intent"].as_str(), obj["amount_refunded"].as_i64()) {
            (Some(pi), Some(total)) => Action::Refunded { payment_intent: pi.to_string(), total_cents: total },
            _ => Action::Ignore,
        },
        "charge.dispute.created" => match obj["payment_intent"].as_str() {
            Some(pi) => Action::Disputed { payment_intent: pi.to_string() },
            None => Action::Ignore,
        },
        _ => Action::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_become_ledger_actions() {
        let paid = json!({"type": "checkout.session.completed", "data": {"object": {
            "id": "cs_1", "payment_status": "paid", "currency": "eur", "amount_total": 1000,
            "client_reference_id": "7", "payment_intent": "pi_1", "metadata": {"account": "7"}}}});
        assert_eq!(
            action(&paid),
            Action::Credit { checkout: "cs_1".into(), account: 7, cents: 1000, payment_intent: Some("pi_1".into()) }
        );
        let mut unpaid = paid.clone();
        unpaid["data"]["object"]["payment_status"] = json!("unpaid");
        assert_eq!(action(&unpaid), Action::Ignore, "SEPA: wait for async_payment_succeeded");
        let mut dollars = paid.clone();
        dollars["data"]["object"]["currency"] = json!("usd");
        assert_eq!(action(&dollars), Action::Ignore);
        let refund =
            json!({"type": "charge.refunded", "data": {"object": {"payment_intent": "pi_1", "amount_refunded": 300}}});
        assert_eq!(action(&refund), Action::Refunded { payment_intent: "pi_1".into(), total_cents: 300 });
        let dispute = json!({"type": "charge.dispute.created", "data": {"object": {"payment_intent": "pi_1"}}});
        assert_eq!(action(&dispute), Action::Disputed { payment_intent: "pi_1".into() });
        assert_eq!(action(&json!({"type": "customer.created"})), Action::Ignore);
    }
}

/// A real payment page in Stripe's test mode: run with the sandbox key in
/// `STRIPE_SECRET_KEY` and `--ignored`.
#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    #[ignore = "talks to Stripe's test mode"]
    fn a_test_mode_checkout_page_is_made() {
        let key = std::env::var("STRIPE_SECRET_KEY").expect("STRIPE_SECRET_KEY");
        assert!(key.starts_with("sk_test_"), "test mode only");
        let api = StripeApi {
            cfg: config::Stripe {
                secret_key: key,
                success_url: "https://example.com/paid".into(),
                cancel_url: "https://example.com/cancelled".into(),
                ..Default::default()
            },
        };
        let url = api.checkout(1, "AS-TEST-0001", 1000).unwrap();
        assert!(url.starts_with("https://checkout.stripe.com/"), "{url}");
    }
}
