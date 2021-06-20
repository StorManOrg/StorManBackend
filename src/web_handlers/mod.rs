use std::str::FromStr;

use actix_web::{error, middleware::errhandlers::ErrorHandlerResponse, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use sysinfo::SystemExt;

pub(crate) mod auth;
pub(crate) mod database;
pub(crate) mod item;
pub(crate) mod location;
pub(crate) mod tag;

#[derive(Serialize, Deserialize, Debug)]
struct ServerInfo {
    supported_api_versions: Vec<u32>,
    server_version: String,
    os: Option<String>,
    os_version: Option<String>,
}

#[actix_web::get("/info")]
async fn get_system_info() -> actix_web::Result<web::Json<ServerInfo>> {
    let system_info = sysinfo::System::new();

    Ok(web::Json(ServerInfo {
        supported_api_versions: vec![1],
        server_version: String::from(option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")),
        os: system_info.get_name(),
        os_version: system_info.get_os_version(),
    }))
}

#[actix_web::get("/teapod")]
async fn teapod() -> HttpResponse {
    HttpResponse::from_error(error::ErrorImATeapot("Your Coffee is in Another Castle!"))
}

pub(crate) fn sanitize_internal_error<B>(mut res: actix_web::dev::ServiceResponse<B>) -> actix_web::Result<ErrorHandlerResponse<B>> {
    res.take_body(); // Delete the http body
    Ok(ErrorHandlerResponse::Response(res))
}

pub(crate) async fn not_implemented() -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::NotImplemented().finish())
}

#[rustfmt::skip]
fn get_param<T>(req: &HttpRequest, field_name: &str, error: &'static str) -> actix_web::Result<T, actix_web::Error> where T: FromStr {
    req.match_info().query(field_name).parse::<T>().map_err(|_| error::ErrorBadRequest(error))
}
