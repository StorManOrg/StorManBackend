use actix_web::{dev, error, web, Error, FromRequest, HttpRequest, HttpResponse, Result};
use futures::{future, Future};
use serde::{Deserialize, Serialize};
use sqlx::{MySqlPool, Row};

use std::{collections::HashMap, pin::Pin, str::FromStr};
use sysinfo::SystemExt;

use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::models::{AuthedUser, Database, Item, Property, Tag, UserCredentials};

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

#[actix_web::get("/auth")]
async fn get_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    get_post_auth(pool, req).await
}

#[actix_web::post("/auth")]
async fn post_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    get_post_auth(pool, req).await
}

async fn get_post_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> Result<HttpResponse> {
    println!("{:?}", req);
    // Query for the user_id with the credentials from the request
    let query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT id FROM users WHERE username = '?' AND password = '?'")
        .bind(&req.username)
        .bind(&req.password)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the user was found and extract the user id,
    // if there was no row found, return an forbidden error (code 403).
    let user_id: u64 = match query {
        Ok(row) => row.try_get(0).unwrap(),
        Err(error) => {
            return Err(match error {
                sqlx::Error::RowNotFound => error::ErrorForbidden("invalid username or password!"),
                _ => error::ErrorInternalServerError(""),
            })
        }
    };

    // Generate a unique session_id and save it in the database.
    // We need a infinite loop here because we want to make sure,
    // that we don't get a duplicate.
    let session_id: String = loop {
        // Generate 8 random alphanumeric (a-zA-Z0-9) characters.
        let session_id: String = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();

        // Try to insert that into the sessions sql table...
        let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO sessions (session_id, user_id) VALUES ('?', ?)")
            .bind(&session_id)
            .bind(&user_id)
            .execute(pool.as_ref())
            .await;

        // ... and check if it succeeded.
        match query {
            Ok(_) => break session_id,

            // If not, try it again (only if the error occurred because of a duplicate).
            Err(error) => {
                return Err(match error {
                    sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => continue,
                    _ => error::ErrorInternalServerError(""),
                });
            }
        }
    };

    SESSION_LIST.lock().unwrap().push(session_id.clone()); // Legacy code support
    Ok(HttpResponse::Ok().json::<HashMap<&str, String>>(collection! {
        "session_id" => session_id
    }))
}

#[actix_web::delete("/auth")]
async fn delete_auth(session: AuthedUser) -> Result<HttpResponse> {
    let mut sessions = SESSION_LIST.lock().unwrap();
    let index = match sessions.iter().position(|entry| entry == &session.session_id) {
        Some(index) => index,
        None => return Err(error::ErrorForbidden("invalid session id!")),
    };

    sessions.remove(index);
    Ok(HttpResponse::Ok().finish())
}

impl FromRequest for AuthedUser {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut dev::Payload) -> Self::Future {
        // We need to clone the pool here because the sql operation later on
        // are async and the compiler can't guarantee us that lifetime of the reference.
        let pool = req.app_data::<web::Data<MySqlPool>>().unwrap().clone();
        let session_id = match req.headers().get("X-StoRe-Session") {
            Some(header) => match header.to_str() {
                Ok(session_id) => session_id.to_string(),
                Err(_) => return Box::pin(future::err(error::ErrorBadRequest("invalid characters in session id!"))),
            },
            None => return Box::pin(future::err(error::ErrorBadRequest("session id is missing!"))),
        };

        // We need a pinned box here because sql operations are async
        // but this is a synchronous function.
        Box::pin(async move {
            let query: Result<AuthedUser, sqlx::Error> = sqlx::query_as::<_, AuthedUser>("SELECT session_id FROM sessions WHERE session_id = '?'")
                .bind(&session_id)
                .fetch_one(pool.as_ref())
                .await;

            match query {
                Ok(auth) => Ok(auth),
                Err(error) => Err(match error {
                    sqlx::Error::RowNotFound => error::ErrorForbidden("invalid session id!"),
                    _ => error::ErrorInternalServerError(""),
                }),
            }
        })
    }
}

#[actix_web::get("/items")]
async fn get_items(_user: AuthedUser) -> Result<web::Json<Vec<Item>>> {
    Ok(web::Json(ITEM_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/item/{item_id}")]
async fn get_item(_user: AuthedUser, req: HttpRequest) -> Result<web::Json<Item>> {
    let item_id: u64 = get_param(&req, "item_id", "item id must be a number!")?;
    if let Some(item) = ITEM_MAP.lock().unwrap().get(&item_id) {
        Ok(web::Json(item.clone()))
    } else {
        Err(error::ErrorNotFound("item not found!"))
    }
}

#[actix_web::put("/item")]
async fn create_item(_user: AuthedUser, mut item: web::Json<Item>) -> Result<HttpResponse> {
    if item.id != 0 {
        return Err(error::ErrorBadRequest("item id must be 0!"));
    }

    // Check if the tags exist
    if !item.tags.iter().all(|&tag_id| TAG_MAP.lock().unwrap().contains_key(&tag_id)) {
        return Err(error::ErrorNotFound("unknown tag id!"));
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
    let item_id: u64 = get_param(&req, "item_id", "item id must be a number!")?;
    if ITEM_MAP.lock().unwrap().contains_key(&item_id) {
        Ok(HttpResponse::Ok().finish())
    } else {
        Err(error::ErrorNotFound("item not found!"))
    }
}

#[actix_web::get("/tags")]
async fn get_tags(_user: AuthedUser) -> Result<web::Json<Vec<Tag>>> {
    Ok(web::Json(TAG_MAP.lock().unwrap().values().cloned().collect()))
}

#[actix_web::get("/tag/{tag_id}")]
async fn get_tag(_user: AuthedUser, req: HttpRequest) -> Result<web::Json<Tag>> {
    let tag_id: u64 = get_param(&req, "tag_id", "tag id must be a number!")?;
    if let Some(tag) = TAG_MAP.lock().unwrap().get(&tag_id) {
        Ok(web::Json(tag.clone()))
    } else {
        Err(error::ErrorNotFound("tag not found!"))
    }
}

#[actix_web::put("/tag")]
async fn create_tag(_user: AuthedUser, mut tag: web::Json<Tag>) -> Result<HttpResponse> {
    if tag.id != 0 {
        return Err(error::ErrorBadRequest("tag id must be 0!"));
    }

    // Check if the tag name is unique
    if TAG_MAP.lock().unwrap().values().any(|map_tag| map_tag.name == tag.name) {
        return Err(error::ErrorNotFound("tag name already exists!"));
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
    let tag_id: u64 = get_param(&req, "tag_id", "tag id must be a number!")?;
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
    let database = sqlx::query_as::<_, Database>("SELECT * FROM item_databases").fetch_all(pool.as_ref()).await.unwrap();
    Ok(web::Json(database))
}

#[actix_web::get("/database/{database_id}")]
async fn get_database(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> Result<web::Json<Database>> {
    let database_id: u64 = get_param(&req, "database_id", "database id must be a number!")?;

    // Query for the object and auto convert it.
    let query: Result<Database, sqlx::Error> = sqlx::query_as::<_, Database>("SELECT * FROM item_databases WHERE id = '?'")
        .bind(database_id)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the query was successful and return the database object,
    // if the database could not be found, set the status code to 404.
    // Should a different kind of error occur, return an Internal Server Error (code: 500).
    match query {
        Ok(database) => Ok(web::Json(database)),
        Err(error) => match error {
            sqlx::Error::RowNotFound => Err(error::ErrorNotFound("Database not found!")),
            _ => Err(error::ErrorInternalServerError("")),
        },
    }
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

#[rustfmt::skip]
fn get_param<T>(req: &HttpRequest, field_name: &str, error: &'static str) -> Result<T, actix_web::Error> where T: FromStr {
    req.match_info().query(field_name).parse::<T>().map_err(|_| error::ErrorBadRequest(error))
}
