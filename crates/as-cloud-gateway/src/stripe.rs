//! Stripe: a hosted payment page for each top-up, and the events that say
//! it was paid, refunded or disputed. Cards and PayPal both come through
//! Checkout (PayPal is switched on in the Stripe Dashboard, not here).
//!
//! A top-up sells the catalog's price for its amount when Stripe has one
//! (lookup key `cloud_topup_<cents>`, product "AutoScene Cloud balance" with
//! its picture), else the amount itself.

use serde_json::Value;

use crate::config;

/// A top-up's price in the Stripe catalog: this and the amount in cents.
pub const TOP_UP_LOOKUP_PREFIX: &str = "cloud_topup_";

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
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .build()
            .into();
        let account = account.to_string();
        let cents_s = cents.to_string();
        let name = format!("AutoScene Cloud balance: {}", as_cloud::euros(cents * as_cloud::MICROS_PER_CENT));
        let price = self.catalog_price(&agent, cents);
        let mut form: Vec<(&str, &str)> = vec![("mode", "payment"), ("line_items[0][quantity]", "1")];
        match &price {
            Some(id) => form.push(("line_items[0][price]", id.as_str())),
            None => form.extend([
                ("line_items[0][price_data][currency]", "eur"),
                ("line_items[0][price_data][unit_amount]", cents_s.as_str()),
                ("line_items[0][price_data][product_data][name]", name.as_str()),
            ]),
        }
        form.extend([
            ("client_reference_id", account.as_str()),
            ("metadata[account]", account.as_str()),
            ("metadata[support_id]", support_id),
            ("metadata[cents]", cents_s.as_str()),
            ("payment_intent_data[metadata][account]", account.as_str()),
            ("payment_intent_data[metadata][support_id]", support_id),
            ("success_url", self.cfg.success_url.as_str()),
            ("cancel_url", self.cfg.cancel_url.as_str()),
        ]);
        if let Some(footer) = &self.cfg.invoice_footer {
            form.push(("invoice_creation[enabled]", "true"));
            form.push(("invoice_creation[invoice_data][footer]", footer));
        }
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

impl StripeApi {
    /// The catalog's price for a top-up of `cents`, asked each time (top-ups
    /// are rare, and an archived price must not linger). Not found or not
    /// reachable: none, and the top-up states its amount itself.
    fn catalog_price(&self, agent: &ureq::Agent, cents: i64) -> Option<String> {
        let key = format!("{TOP_UP_LOOKUP_PREFIX}{cents}");
        let mut resp = agent
            .get("https://api.stripe.com/v1/prices")
            .query("lookup_keys[]", &key)
            .query("active", "true")
            .header("Authorization", &format!("Bearer {}", self.cfg.secret_key))
            .call()
            .map_err(|e| tracing::warn!("looking up the price {key}: {e}"))
            .ok()?;
        let raw = resp.body_mut().read_to_vec().ok()?;
        if !resp.status().is_success() {
            tracing::warn!("looking up the price {key}: Stripe answered {}", resp.status());
            return None;
        }
        let found = pick_price(&serde_json::from_slice(&raw).ok()?, cents);
        if found.is_none() {
            tracing::info!("no catalog price {key}: the top-up states its amount");
        }
        found
    }
}

/// The price in a `GET /v1/prices` list that sells exactly `cents` euro
/// cents once: anything else (another amount or currency, a subscription,
/// tiers, pay-what-you-want) is not a top-up's price.
pub fn pick_price(list: &Value, cents: i64) -> Option<String> {
    list["data"]
        .as_array()?
        .iter()
        .find(|p| {
            p["active"] == true
                && p["currency"] == "eur"
                && p["type"] == "one_time"
                && p["billing_scheme"] == "per_unit"
                && p["unit_amount"].as_i64() == Some(cents)
                && p["transform_quantity"].is_null()
        })
        .and_then(|p| p["id"].as_str())
        .map(str::to_string)
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

    #[test]
    fn a_top_up_takes_only_a_catalog_price_that_sells_its_exact_amount() {
        let price = |over: Value| {
            let mut p = json!({"id": "price_ok", "active": true, "currency": "eur", "type": "one_time",
                "billing_scheme": "per_unit", "unit_amount": 1000, "transform_quantity": null});
            for (k, v) in over.as_object().unwrap() {
                p[k] = v.clone();
            }
            p
        };
        let list = |prices: Vec<Value>| json!({"object": "list", "data": prices});
        assert_eq!(pick_price(&list(vec![price(json!({}))]), 1000).as_deref(), Some("price_ok"));
        assert_eq!(pick_price(&list(vec![price(json!({}))]), 500), None, "another amount");
        for wrong in [
            json!({"currency": "usd"}),
            json!({"type": "recurring"}),
            json!({"billing_scheme": "tiered"}),
            json!({"unit_amount": null}),
            json!({"active": false}),
            json!({"transform_quantity": {"divide_by": 2, "round": "up"}}),
        ] {
            assert_eq!(pick_price(&list(vec![price(wrong.clone())]), 1000), None, "{wrong}");
        }
        let both = list(vec![price(json!({"id": "price_usd", "currency": "usd"})), price(json!({}))]);
        assert_eq!(pick_price(&both, 1000).as_deref(), Some("price_ok"));
        assert_eq!(pick_price(&json!({"error": {"message": "no"}}), 1000), None);
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

    /// The sandbox has the live catalog's top-up prices (same lookup keys).
    #[test]
    #[ignore = "talks to Stripe's test mode"]
    fn the_catalog_has_a_price_for_every_top_up() {
        let key = std::env::var("STRIPE_SECRET_KEY").expect("STRIPE_SECRET_KEY");
        assert!(key.starts_with("sk_test_"), "test mode only");
        let api = StripeApi { cfg: config::Stripe { secret_key: key, ..Default::default() } };
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        for cents in &as_cloud::TOP_UPS_CENTS {
            let id = api.catalog_price(&agent, *cents);
            assert!(id.as_deref().is_some_and(|i| i.starts_with("price_")), "{cents}: {id:?}");
        }
        assert_eq!(api.catalog_price(&agent, 777), None, "no price for an amount not on offer");
    }
}
