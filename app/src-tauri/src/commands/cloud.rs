//! Settings → Cloud: the balance, codes, top-ups and the switch that sends
//! analyses to AutoScene Cloud. Each call may wait for the network and the
//! keychain, so none runs on the UI's thread.

use as_core::Msg;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

use crate::cloud::{self, CloudError};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::CloudChanged;
use crate::state::AppState;

/// What one minute of video costs, as people read it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloudPrices {
    pub standard: String,
    pub fast: String,
    pub best: String,
}

/// A top-up on offer.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloudTopUp {
    pub cents: i32,
    pub label: String,
}

/// This computer's cloud account as Settings shows it: a balance, never a key.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloudAccount {
    /// "€4.99".
    pub balance: String,
    /// About how many minutes of a standard scan it pays for.
    pub minutes: u32,
    pub support_id: String,
    /// Scans are free and unlimited until then (unix seconds).
    pub beta_until: Option<f64>,
    pub blocked: bool,
    pub paused: bool,
    pub prices: CloudPrices,
    pub top_ups: Vec<CloudTopUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    /// A gateway is set up for this build (or by the person).
    pub available: bool,
    /// Analyses run in the cloud.
    pub enabled: bool,
    pub account: Option<CloudAccount>,
    /// Why the account couldn't be read.
    pub error: Option<Msg>,
}

fn account_view(a: as_cloud::Account) -> CloudAccount {
    let minutes = if a.prices.standard > 0 { (a.balance.max(0) / a.prices.standard) as u32 } else { 0 };
    CloudAccount {
        balance: as_cloud::euros(a.balance),
        minutes,
        support_id: a.support_id,
        beta_until: a.beta_until.map(|t| t as f64),
        blocked: a.blocked,
        paused: a.paused,
        prices: CloudPrices {
            standard: per_minute(a.prices.standard),
            fast: per_minute(a.prices.fast),
            best: per_minute(a.prices.best),
        },
        top_ups: a
            .top_ups
            .into_iter()
            .map(|c| CloudTopUp { cents: c as i32, label: as_cloud::euros(c * as_cloud::MICROS_PER_CENT) })
            .collect(),
    }
}

/// A price per minute, to the tenth of a cent when it needs it ("€0.005").
fn per_minute(m: as_cloud::Micros) -> String {
    let cents = m as f64 / as_cloud::MICROS_PER_CENT as f64;
    if (cents - cents.round()).abs() < 1e-9 {
        format!("€{:.2}", cents / 100.0)
    } else {
        format!("€{:.4}", cents / 100.0).trim_end_matches('0').to_string()
    }
}

fn status(state: &AppState) -> CloudStatus {
    let available = cloud::gateway(&state.library).is_some();
    let enabled = cloud::enabled(&state.library);
    if !available {
        return CloudStatus { available, enabled, account: None, error: None };
    }
    match cloud::account(&state.library) {
        Ok(a) => CloudStatus { available, enabled, account: Some(account_view(a)), error: None },
        Err(e) => {
            log::warn!("cloud account: {}", e.detail());
            CloudStatus { available, enabled, account: None, error: Some(e.msg()) }
        }
    }
}

/// A refusal people read (a code that doesn't exist, the cloud offline).
fn refused(m: Msg) -> AppError {
    AppError::new(ErrorCode::Invalid, m)
}

async fn blocking<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(&AppState) -> T + Send + 'static) -> T {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .expect("the cloud call didn't panic")
}

/// The balance and what the cloud offers.
#[tauri::command]
#[specta::specta]
pub async fn cloud_status(app: AppHandle) -> CloudStatus {
    blocking(&app, status).await
}

/// Send analyses to the cloud, or keep them on this computer.
#[tauri::command]
#[specta::specta]
pub async fn cloud_set_enabled(app: AppHandle, on: bool) -> AppResult<CloudStatus> {
    blocking(&app, move |state| {
        cloud::set_enabled(&state.library, on).map_err(|e| {
            log::warn!("cloud setting: {e}");
            AppError::internal(as_core::msg!("cloud.settingFailed", "The setting couldn't be saved."))
        })?;
        Ok(status(state))
    })
    .await
}

/// Redeem a top-up code (or a beta pass).
#[tauri::command]
#[specta::specta]
pub async fn cloud_redeem(app: AppHandle, code: String) -> AppResult<CloudStatus> {
    let app2 = app.clone();
    let r = blocking(&app, move |state| -> Result<CloudStatus, CloudError> {
        cloud::client(&state.library)?.redeem(&code)?;
        Ok(status(state))
    })
    .await;
    let _ = CloudChanged.emit(&app2);
    r.map_err(|e| {
        log::info!("redeeming a code: {}", e.detail());
        refused(e.msg())
    })
}

/// Open Stripe's payment page for a top-up of `cents` in the browser. The
/// balance grows once Stripe tells the gateway it was paid.
#[tauri::command]
#[specta::specta]
pub async fn cloud_add_funds(app: AppHandle, cents: i32) -> AppResult<()> {
    let url = blocking(&app, move |state| -> Result<String, CloudError> {
        Ok(cloud::client(&state.library)?.checkout(i64::from(cents))?)
    })
    .await
    .map_err(|e| {
        log::warn!("payment page: {}", e.detail());
        refused(e.msg())
    })?;
    app.opener().open_url(&url, None::<&str>).map_err(|e| {
        log::warn!("opening the payment page: {e}");
        AppError::internal(as_core::msg!("cloud.browser", "The payment page couldn't be opened in your browser."))
    })
}

/// Point AutoScene at another gateway (empty: the built-in one).
#[tauri::command]
#[specta::specta]
pub async fn cloud_set_gateway(app: AppHandle, url: String) -> AppResult<CloudStatus> {
    blocking(&app, move |state| {
        let url = url.trim();
        // https anywhere; plain http only to this computer (a gateway run for testing).
        let ok = url.is_empty()
            || url::Url::parse(url).is_ok_and(|u| match u.scheme() {
                "https" => u.host().is_some(),
                "http" => matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
                _ => false,
            });
        if !ok {
            return Err(refused(as_core::msg!("cloud.gatewayHttps", "The address must start with https://.")));
        }
        state.library.shared_db().set_setting(cloud::KEY_GATEWAY, url).map_err(|e| {
            log::warn!("cloud gateway: {e}");
            AppError::internal(as_core::msg!("cloud.settingFailed", "The setting couldn't be saved."))
        })?;
        Ok(status(state))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_read_as_people_expect() {
        assert_eq!(per_minute(10_000), "€0.01");
        assert_eq!(per_minute(5_000), "€0.005");
        assert_eq!(per_minute(12_500), "€0.0125");
    }
}
