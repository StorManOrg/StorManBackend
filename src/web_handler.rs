use actix_web::{error, web, HttpRequest, HttpResponse, Result};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use sysinfo::SystemExt;

use crate::models::{Item, Property, Tag};

use crate::collection;
use lazy_static::lazy_static;
use std::sync::Mutex;

lazy_static! {
    static ref ITEM_MAP: Mutex<HashMap<u64, Item>> = Mutex::new(collection! {
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
    });
    static ref TAG_MAP: Mutex<HashMap<u64, Tag>> = Mutex::new(collection! {
        1 => Tag {
            id: 1,
            name: String::from("test-tag"),
            color: 345156,
            icon: Some(24),
        },
        2 => Tag {
            id: 2,
            name: String::from("foo-tag"),
            color: 78354,
            icon: Some(23),
        }
    });
}

#[actix_web::get("/items")]
async fn get_items() -> Result<web::Json<Vec<Item>>> {
    Ok(web::Json(ITEM_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/item/{item_id}")]
async fn get_item(req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");
    if let Some(item) = ITEM_MAP.lock().unwrap().get(&item_id) {
        Ok(web::Json(item.clone()))
    } else {
        Err(error::ErrorNotFound("Item not found!"))
    }
}

#[actix_web::put("/item")]
async fn create_item(mut item: web::Json<Item>) -> Result<HttpResponse> {
    if item.id != 0 {
        return Err(error::ErrorBadRequest("Item id must be 0!"));
    }

    // Check if the tags exist
    if !item.tags.iter().all(|&tag_id| TAG_MAP.lock().unwrap().contains_key(&tag_id)) {
        return Err(error::ErrorNotFound("Unknown tag id!"));
    }

    // Generate a new random id
    item.id = rand::random();
    while ITEM_MAP.lock().unwrap().contains_key(&item.id) {
        item.id = rand::random::<u16>() as u64;
    }

    ITEM_MAP.lock().unwrap().insert(item.id, item.clone());
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "item_id" => item.id
    }))
}

#[actix_web::delete("/item/{item_id}")]
async fn delete_item(req: HttpRequest) -> Result<HttpResponse> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");
    if ITEM_MAP.lock().unwrap().contains_key(&item_id) {
        Ok(HttpResponse::Ok().body(actix_web::body::Body::None))
    } else {
        Err(error::ErrorNotFound("Item not found!"))
    }
}

#[actix_web::get("/tags")]
async fn get_tags() -> Result<web::Json<Vec<Tag>>> {
    Ok(web::Json(TAG_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/tag/{tag_id}")]
async fn get_tag(req: HttpRequest) -> Result<web::Json<Tag>> {
    let tag_id: u64 = req.match_info().query("tag_id").parse().expect("Not a number");
    if let Some(tag) = TAG_MAP.lock().unwrap().get(&tag_id) {
        Ok(web::Json(tag.clone()))
    } else {
        Err(error::ErrorNotFound("Tag not found!"))
    }
}

#[actix_web::put("/tag")]
async fn create_tag(mut tag: web::Json<Tag>) -> Result<HttpResponse> {
    if tag.id != 0 {
        return Err(error::ErrorBadRequest("Tag id must be 0!"));
    }

    // Check if the tag name is unique
    if TAG_MAP.lock().unwrap().values().any(|map_tag| map_tag.name == tag.name) {
        return Err(error::ErrorNotFound("Tag name already exists!"));
    }

    // Generate a new random id
    tag.id = rand::random();
    while ITEM_MAP.lock().unwrap().contains_key(&tag.id) {
        tag.id = rand::random::<u16>() as u64;
    }

    TAG_MAP.lock().unwrap().insert(tag.id, tag.clone());
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "tag_id" => tag.id
    }))
}

#[actix_web::delete("/tag/{tag_id}")]
async fn delete_tag(req: HttpRequest) -> Result<HttpResponse> {
    let tag_id: u64 = req.match_info().query("tag_id").parse().expect("Not a number");
    if !TAG_MAP.lock().unwrap().contains_key(&tag_id) {
        return Err(error::ErrorNotFound("Tag not found!"));
    }

    // Check if items depend on this tag
    // Stream: get all items -> map them to an array of tag ids -> check if there is a match
    if ITEM_MAP.lock().unwrap().values().flat_map(|item| &item.tags).any(|&map_tag_id| map_tag_id == tag_id) {
        return Err(error::ErrorConflict("There is an item that depends on this tag!"));
    }

    TAG_MAP.lock().unwrap().remove(&tag_id);
    Ok(HttpResponse::Ok().body(actix_web::body::Body::None))
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerInfo {
    api_version: u32,
    server_version: String,
    os: Option<String>,
    os_version: Option<String>,
}

#[actix_web::get("/info")]
async fn get_system_info() -> Result<web::Json<ServerInfo>> {
    let system_info = sysinfo::System::new();

    Ok(web::Json(ServerInfo {
        api_version: 1,
        server_version: String::from(option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")),
        os: system_info.get_name(),
        os_version: system_info.get_os_version(),
    }))
}
