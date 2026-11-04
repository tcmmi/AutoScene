//! A plugin's requests over wasi:http. AutoScene sends each itself (never
//! wasmtime's own client, which resolves and connects without a check);
//! this AutoScene sends none: every request is refused as denied, whatever
//! the plugin's grants, until the network guard comes.

use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

/// What a request of the plugin is answered with.
type Sent = Result<(http::Response<WasiBody>, Box<dyn Future<Output = Result<(), Error>> + Send>), Error>;

/// AutoScene's side of wasi:http for one run.
pub(crate) struct Guard;

impl WasiHttpHooks for Guard {
    fn send_request(
        &mut self,
        _request: http::Request<WasiBody>,
        _options: Option<RequestOptions>,
        _fut: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<dyn Future<Output = Sent> + Send> {
        Box::new(async { Err(Error::HttpRequestDenied) })
    }
}
