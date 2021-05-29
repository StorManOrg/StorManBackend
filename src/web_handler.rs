use actix_web::{dev, error, web, Error, FromRequest, HttpRequest, HttpResponse, Result};
use futures_util::future::{err, ok, Ready};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, Row};

use std::collections::HashMap;
use sysinfo::SystemExt;

use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::models::{Database, Item, Property, Tag};

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
    static ref SESSION_LIST: Mutex<Vec<String>> = Mutex::new(vec![]);
}

#[derive(Serialize, Deserialize, Debug)]
struct UserCredentials {
    username: String,
    password: String,
}

#[actix_web::get("/auth")]
async fn get_auth(req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    get_post_auth(req)
}

#[actix_web::post("/auth")]
async fn post_auth(req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    get_post_auth(req)
}

fn get_post_auth(req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    if !(req.username == "admin" && req.password == "123") {
        return Err(error::ErrorForbidden("Invalid username or password!"));
    }

    let mut session_id: String = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();
    while SESSION_LIST.lock().unwrap().contains(&session_id) {
        session_id = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();
    }

    SESSION_LIST.lock().unwrap().push(session_id.clone());
    Ok(HttpResponse::Ok().json::<HashMap<&str, String>>(collection! {
        "session_id" => session_id
    }))
}

#[actix_web::delete("/auth")]
async fn delete_auth(session: AuthedUser) -> Result<HttpResponse> {
    let mut sessions = SESSION_LIST.lock().unwrap();
    let index = sessions.iter().position(|entry| entry == &session.session_id).expect("session not found!");
    sessions.remove(index);

    Ok(HttpResponse::Ok().finish())
}

/// If this struct is a parameter in an actix service,
/// it becomes a protected service
#[derive(Serialize, Deserialize, Debug)]
struct AuthedUser {
    session_id: String,
}

impl FromRequest for AuthedUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        // FIXME: wtf is this
        let session_id = if let Some(session_id) = req.headers().get("X-StoRe-Session") {
            if let Ok(valid_seesion_id) = session_id.to_str() {
                valid_seesion_id.to_string()
            } else {
                return err(error::ErrorBadRequest("Invalid characters in session id!"));
            }
        } else {
            return err(error::ErrorBadRequest("Session id is missing!"));
        };

        if SESSION_LIST.lock().unwrap().contains(&session_id) {
            ok(AuthedUser { session_id })
        } else {
            err(error::ErrorForbidden("Invalid session id!"))
        }
    }
}

#[actix_web::get("/items")]
async fn get_items(_user: AuthedUser) -> Result<web::Json<Vec<Item>>> {
    Ok(web::Json(ITEM_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/item/{item_id}")]
async fn get_item(_user: AuthedUser, req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");
    if let Some(item) = ITEM_MAP.lock().unwrap().get(&item_id) {
        Ok(web::Json(item.clone()))
    } else {
        Err(error::ErrorNotFound("Item not found!"))
    }
}

#[actix_web::put("/item")]
async fn create_item(_user: AuthedUser, mut item: web::Json<Item>) -> Result<HttpResponse> {
    if item.id != 0 {
        return Err(error::ErrorBadRequest("Item id must be 0!"));
    }

    // Check if the tags exist
    if !item.tags.iter().all(|&tag_id| TAG_MAP.lock().unwrap().contains_key(&tag_id)) {
        return Err(error::ErrorNotFound("Unknown tag id!"));
    }

    // Generate a new random id
    item.id = rand::random::<u16>() as u64;
    while ITEM_MAP.lock().unwrap().contains_key(&item.id) {
        item.id = rand::random::<u16>() as u64;
    }

    ITEM_MAP.lock().unwrap().insert(item.id, item.clone());
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "item_id" => item.id
    }))
}

#[actix_web::delete("/item/{item_id}")]
async fn delete_item(_user: AuthedUser, req: HttpRequest) -> Result<HttpResponse> {
    let item_id: u64 = req.match_info().query("item_id").parse().expect("Not a number");
    if ITEM_MAP.lock().unwrap().contains_key(&item_id) {
        Ok(HttpResponse::Ok().finish())
    } else {
        Err(error::ErrorNotFound("Item not found!"))
    }
}

#[actix_web::get("/tags")]
async fn get_tags(_user: AuthedUser) -> Result<web::Json<Vec<Tag>>> {
    Ok(web::Json(TAG_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/tag/{tag_id}")]
async fn get_tag(_user: AuthedUser, req: HttpRequest) -> Result<web::Json<Tag>> {
    let tag_id: u64 = req.match_info().query("tag_id").parse().expect("Not a number");
    if let Some(tag) = TAG_MAP.lock().unwrap().get(&tag_id) {
        Ok(web::Json(tag.clone()))
    } else {
        Err(error::ErrorNotFound("Tag not found!"))
    }
}

#[actix_web::put("/tag")]
async fn create_tag(_user: AuthedUser, mut tag: web::Json<Tag>) -> Result<HttpResponse> {
    if tag.id != 0 {
        return Err(error::ErrorBadRequest("Tag id must be 0!"));
    }

    // Check if the tag name is unique
    if TAG_MAP.lock().unwrap().values().any(|map_tag| map_tag.name == tag.name) {
        return Err(error::ErrorNotFound("Tag name already exists!"));
    }

    // Generate a new random id
    tag.id = rand::random::<u16>() as u64;
    while ITEM_MAP.lock().unwrap().contains_key(&tag.id) {
        tag.id = rand::random::<u16>() as u64;
    }

    TAG_MAP.lock().unwrap().insert(tag.id, tag.clone());
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "tag_id" => tag.id
    }))
}

#[actix_web::delete("/tag/{tag_id}")]
async fn delete_tag(_user: AuthedUser, req: HttpRequest) -> Result<HttpResponse> {
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
    Ok(HttpResponse::Ok().finish())
}

#[actix_web::get("/databases")]
async fn get_databases(pool: web::Data<MySqlPool>, _user: AuthedUser) -> Result<web::Json<Vec<Database>>> {
    let database = sqlx::query("SELECT * FROM item_databases").fetch_all(pool.as_ref()).await.unwrap();
    Ok(web::Json(
        database
            .iter()
            .map(|row| Database {
                id: row.try_get::<i64, _>(0).unwrap() as u64,
                name: row.try_get::<String, _>(1).unwrap(),
            })
            .collect(),
    ))
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

pub(crate) async fn not_implemented() -> Result<HttpResponse> {
    Ok(HttpResponse::NotImplemented().finish())
}
