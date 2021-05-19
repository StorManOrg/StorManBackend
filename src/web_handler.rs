use actix_web::{error, get, web, HttpRequest, Result};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use sysinfo::SystemExt;

use crate::models::{Item, Property, Tag};

macro_rules! collection {
    // map-like
    ($($k:expr => $v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::array::IntoIter::new([$(($k, $v),)*]))
    };
    // set-like
    ($($v:expr),* $(,)?) => {
        std::iter::Iterator::collect(std::array::IntoIter::new([$($v,)*]))
    };
}

lazy_static! {
    static ref ITEM_MAP: HashMap<u64, Item> = collection! {
        1 => Item {
            id: 1,
            name: String::from("TestItem"),
            description: String::from("TestItem please ignore"),
            image: String::from("http://IP/PATH/img.png"),
            location: 1,
            tags: vec![1, 2],
            amount: 25,
            properties_custom: vec![
                Property {
                    id: 1,
                    name: String::from("mein-wert"),
                    value: String::from("Hello World"),
                    display_type: None,
                    min: None,
                    max: None,
                },
                Property {
                    id: 2,
                    name: String::from("Mein EAN13"),
                    value: String::from("1568745165912"),
                    display_type: Some(String::from("ean13")),
                    min: None,
                    max: None,
                },
            ],
            properties_internal: vec![Property {
                id: 3,
                name: String::from("price"),
                value: String::from("32"),
                display_type: None,
                min: None,
                max: None,
            }],
            attachments: collection! {
                String::from("doc1") => String::from("http://IP/PATH/file.ext")
            },
            last_edited: 3165416541646341,
            created: 3165416541646141,
        },
        2 => Item {
            id: 2,
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
        }
    };
    static ref TAG_MAP: HashMap<u64, Tag> = collection! {
        1 => Tag {
            id: 1,
            name: String::from("test-tag"),
            color: 345156,
            icon: 24,
        },
        2 => Tag {
            id: 2,
            name: String::from("foo-tag"),
            color: 78354,
            icon: 23,
        }
    };
}

#[get("/items")]
async fn get_items() -> Result<web::Json<Vec<Item>>> {
    Ok(web::Json(ITEM_MAP.values().cloned().collect()))
}

#[get("/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");
    if let Some(item) = ITEM_MAP.get(&item_id) {
        Ok(web::Json(item.clone()))
    } else {
        Err(error::ErrorNotFound("Item not found!"))
    }
}

#[get("/tags")]
async fn get_tags() -> Result<web::Json<Vec<Tag>>> {
    Ok(web::Json(TAG_MAP.values().cloned().collect()))
}

#[get("/tag/{tag_id}")]
async fn get_tag(req: HttpRequest) -> Result<web::Json<Tag>> {
    let tag_id: u64 = req.match_info().query("tag_id").parse().expect("Not a number");
    if let Some(tag) = TAG_MAP.get(&tag_id) {
        Ok(web::Json(tag.clone()))
    } else {
        Err(error::ErrorNotFound("Tag not found!"))
    }
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
