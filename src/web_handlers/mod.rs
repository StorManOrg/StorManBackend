use std::str::FromStr;

use actix_web::{dev::ServiceResponse, error, middleware::ErrorHandlerResponse, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

use log::error;
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
    os: String,
    os_version: String,
}

#[actix_web::get("/info")]
async fn get_system_info() -> actix_web::Result<web::Json<ServerInfo>> {
    let system_info = sysinfo::System::new();

    Ok(web::Json(ServerInfo {
        supported_api_versions: vec![1],
        server_version: option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "unknown").to_owned(),
        os: system_info.name().unwrap_or_else(|| "unknown".to_owned()),
        os_version: system_info.os_version().unwrap_or_else(|| "unknown".to_owned()),
    }))
}

#[actix_web::get("/teapod")]
async fn teapod() -> HttpResponse {
    HttpResponse::from_error(error::ErrorImATeapot("Your Coffee is in Another Castle!"))
}

pub(crate) fn sanitize_internal_error<B>(res: ServiceResponse<B>) -> actix_web::Result<ErrorHandlerResponse<B>> {
    let (request, old_response) = res.into_parts();
    let new_response = actix_web::error::ErrorInternalServerError("");
    let service_response = ServiceResponse::from_err(new_response, request);

    if let Some(err) = old_response.error() {
        error!("Internal Server Error: {}", err);
    }

    Ok(ErrorHandlerResponse::Response(service_response.map_into_right_body()))
}

pub(crate) async fn not_implemented() -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::NotImplemented().finish())
}

/// Extract the specified field from a request
/// or return error 400 (Bad Request) with the
/// provided error message.
#[rustfmt::skip]
fn get_param<T: FromStr>(req: &HttpRequest, field_name: &str, error: &'static str) -> actix_web::Result<T, actix_web::Error> {
    req.match_info().query(field_name).parse::<T>().map_err(|_| error::ErrorBadRequest(error))
}
