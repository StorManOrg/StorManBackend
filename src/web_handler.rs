use actix_web::{get, web, HttpRequest, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use sysinfo::SystemExt;

use crate::models::Item;

#[get("/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");

    Ok(web::Json(Item {
        id: Some(item_id),
        name: String::from("Test Item 3"),
        description: String::from("Sample Description"),
        image: String::from("fejfeifji"),
        location: 5,
        tags: vec![6, 3],
        amount: 29,
        properties_internal: vec![],
        properties_custom: vec![],
        attachments: HashMap::new(),
        last_edited: 637463746,
        created: 989343,
    }))
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerInfo {
    api_version: u32,
    server_version: String,
    os: Option<String>,
    os_version: Option<String>,
}

#[get("/info")]
async fn get_system_info() -> Result<web::Json<ServerInfo>> {
    let system_info = sysinfo::System::new();

    Ok(web::Json(ServerInfo {
        api_version: 1,
        server_version: String::from(option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")),
        os: system_info.get_name(),
        os_version: system_info.get_os_version(),
    }))
}
