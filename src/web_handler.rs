use actix_web::{error, middleware::errhandlers::ErrorHandlerResponse, web, FromRequest, HttpRequest, HttpResponse};
use futures::future;
use serde::{Deserialize, Serialize};
use sqlx::{types::chrono, MySqlPool, Row};

use std::{collections::HashMap, pin::Pin, str::FromStr};
use sysinfo::SystemExt;

use rand::distributions::Alphanumeric;
use rand::Rng;

use crate::models::{AuthedUser, Database, Item, Location, Property, Tag, UserCredentials};

use crate::collection;

#[rustfmt::skip]
#[actix_web::route("/auth", method="GET", method="POST")]
async fn get_post_auth(pool: web::Data<MySqlPool>, req: web::Json<UserCredentials>) -> actix_web::Result<HttpResponse> {
    println!("{:?}", req);
    // Query for the user_id with the credentials from the request
    let query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT id FROM users WHERE username = ? AND password = ?")
        .bind(&req.username)
        .bind(&req.password)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the user was found and extract the user id,
    // if there was no row found, return an forbidden error (code 403).
    let user_id: u64 = match query {
        Ok(row) => row.try_get(0).unwrap(),
        Err(error) => return Err(match error {
            sqlx::Error::RowNotFound => error::ErrorForbidden("invalid username or password!"),
            _ => error::ErrorInternalServerError(error),
        }),
    };

    // Generate a unique session_id and save it in the database.
    // We need a infinite loop here because we want to make sure,
    // that we don't get a duplicate.
    let session_id: String = loop {
        // Generate 8 random alphanumeric (a-z,A-Z,0-9) characters.
        let session_id: String = rand::thread_rng().sample_iter(&Alphanumeric).take(8).map(char::from).collect();

        // Try to insert that into the sessions sql table...
        let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO sessions (session_id, user_id) VALUES (?, ?)")
            .bind(&session_id)
            .bind(&user_id)
            .execute(pool.as_ref())
            .await;

        // ... and check if it succeeded.
        match query {
            Ok(_) => break session_id,

            // If not, try it again (but only if the error occurred because of a duplicate).
            Err(error) => {
                return Err(match error {
                    sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => continue,
                    _ => error::ErrorInternalServerError(error),
                });
            }
        }
    };

    Ok(HttpResponse::Ok().json::<HashMap<&str, String>>(collection! {
        "session_id" => session_id
    }))
}

#[actix_web::delete("/auth")]
async fn delete_auth(pool: web::Data<MySqlPool>, session: AuthedUser) -> actix_web::Result<HttpResponse> {
    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM sessions WHERE session_id = ?")
        .bind(&session.session_id)
        .execute(pool.as_ref())
        .await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the item didn't even exist!
    // Technically this can't happen, because we made sure
    // the user's session is valid before we even entered
    // this function. (See #AuthedUser for more)
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorForbidden("invalid session id!"));
    }

    Ok(HttpResponse::Ok().finish())
}

impl FromRequest for AuthedUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn futures::Future<Output = Result<Self, Self::Error>>>>;
    type Config = ();

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
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
            let query: Result<AuthedUser, sqlx::Error> = sqlx::query_as::<_, AuthedUser>("SELECT session_id FROM sessions WHERE session_id = ?")
                .bind(&session_id)
                .fetch_one(pool.as_ref())
                .await;

            match query {
                Ok(auth) => Ok(auth),
                Err(error) => Err(match error {
                    sqlx::Error::RowNotFound => error::ErrorForbidden("invalid session id!"),
                    _ => error::ErrorInternalServerError(error),
                }),
            }
        })
    }
}

#[actix_web::get("/items")]
async fn get_items(pool: web::Data<MySqlPool>, _user: AuthedUser) -> actix_web::Result<web::Json<Vec<Item>>> {
    let mut items: HashMap<u64, Item> = sqlx::query("SELECT * FROM items")
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .map(sqlrow_to_basic_item)
        .map(|item| (item.id, item))
        .collect();

    // (Look at the "attachments" query for an explanation)
    sqlx::query("SELECT item_id, tag_id FROM item_tags")
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .for_each(|row| {
            let item_id: u64 = row.get(0);
            let tag_id: u64 = row.get(1);

            if let Some(item) = items.get_mut(&item_id) {
                item.tags.push(tag_id);
            }
        });

    // (Look at the "attachments" query for an explanation)
    sqlx::query("SELECT item_id, is_custom, name, value FROM item_properties")
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .for_each(|row| {
            let item_id: u64 = row.get(0);
            let is_custom: bool = row.get(1);
            let name: String = row.get(2);
            let value: String = row.get(3);

            if let Some(item) = items.get_mut(&item_id) {
                // Get the internal or custom properties list depending in 'is_custom'
                let properties: &mut Vec<Property> = if is_custom { &mut item.properties_custom } else { &mut item.properties_internal };

                properties.push(Property { name, value });
            }
        });

    // Insert the list of sql rows into the item attachments map.
    // The sql rows only contain one part of the final map
    // so we need to go throw the hole list piece by piece.
    sqlx::query("SELECT item_id, name, url FROM item_attachments")
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .for_each(|row| {
            // Get the stuff from the sql row
            let item_id: u64 = row.get(0);
            let name: String = row.get(1);
            let url: String = row.get(2);

            // Get the attachments map and insert the name and url.
            // Technically getting the item could fail if a invalid
            // item id is in the attachments sql table, but this
            // should be prevented by the foreign keys in the sql table.
            if let Some(item) = items.get_mut(&item_id) {
                item.attachments.insert(name, url);
            }
        });

    // Convert the map back into an array:
    // Map<u64, Item> -> Vec<Item>
    Ok(web::Json(items.into_iter().map(|(_, item)| item).collect()))
}

#[actix_web::get("/item/{item_id}")]
async fn get_item(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<web::Json<Item>> {
    let item_id: u64 = get_param(&req, "item_id", "item id must be a number!")?;

    let query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT * FROM items WHERE id = ?").bind(item_id).fetch_one(pool.as_ref()).await;

    // Check if the query was successful, convert the row into an item.
    // If the item could not be found, set the status code to 404.
    // Should a different kind of error occur, return an Internal Server Error (code: 500).
    let mut item = match query {
        Ok(row) => sqlrow_to_basic_item(&row),
        Err(error) => {
            return Err(match error {
                sqlx::Error::RowNotFound => error::ErrorNotFound("item not found!"),
                _ => error::ErrorInternalServerError(error),
            })
        }
    };

    item.tags = sqlx::query("SELECT tag_id FROM item_tags WHERE item_id = ?")
        .bind(&item.id)
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    sqlx::query("SELECT is_custom, name, value FROM item_properties WHERE item_id = ?")
        .bind(&item.id)
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .for_each(|row| {
            let is_custom: bool = row.get(0);
            let name: String = row.get(1);
            let value: String = row.get(2);

            // Get the internal or custom properties list depending in 'is_custom'
            let properties: &mut Vec<Property> = if is_custom { &mut item.properties_custom } else { &mut item.properties_internal };

            properties.push(Property { name, value });
        });

    item.attachments = sqlx::query("SELECT name, url FROM item_attachments WHERE item_id = ?")
        .bind(&item.id)
        .fetch_all(pool.as_ref())
        .await
        .unwrap()
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();

    Ok(web::Json(item))
}

#[rustfmt::skip]
#[actix_web::put("/item")]
async fn put_item(pool: web::Data<MySqlPool>, _user: AuthedUser, item: web::Json<Item>) -> actix_web::Result<HttpResponse> {
    if item.id != 0 {
        return Err(error::ErrorBadRequest("item id must be 0!"));
    }

    // We need to make a transaction here for two reasons:
    // 1. we want to make 2 queries that relate to each other
    // 2. if something goes wrong along the function,
    //    all changes to the database will be discarded.
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // First insert the object into the sql table...
    let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO items (name,description,image,location_id,amount,last_edited,created) VALUES (?,?,?,?,?,?,?)")
            .bind(&item.name)
            .bind(&item.description)
            .bind(&item.image)
            .bind(&item.location)
            .bind(&item.amount)
            .bind(&chrono::NaiveDateTime::from_timestamp(item.last_edited as i64, 0))
            .bind(&chrono::NaiveDateTime::from_timestamp(item.created as i64, 0))
            .execute(&mut tx)
            .await;

    // ...then make sure it didn't fail.
    if let Err(error) = insertion_query {
        return Err(match error {
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a item with this name!"),
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown location id!"),
            _ => error::ErrorInternalServerError(error),
        });
    }

    // After that we need to get the autogenerated id from the table.
    let selection_query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT LAST_INSERT_ID()").fetch_one(&mut tx).await;

    // If we encounter an error then return status 500,
    // if not, extract the id from the query.
    let item_id: u64 = selection_query.map_err(error::ErrorInternalServerError)?.try_get(0).unwrap();

    // (Look at the "attachments" query for an explanation)
    if !item.tags.is_empty() {
        let tag_sql: String = format!("INSERT INTO item_tags (item_id,tag_id) VALUES (?,?){}", ", (?,?)".repeat(item.tags.len() - 1));

        let mut tag_insertion = sqlx::query(tag_sql.as_str());
        for tag in &item.tags {
            tag_insertion = tag_insertion.bind(&item_id).bind(tag);
        }

        // Execute the query and check for errors.
        if let Err(error) = tag_insertion.execute(&mut tx).await {
            return Err(match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown tag id!"),
                _ => error::ErrorInternalServerError(error),
            });
        }
    }

    // (Look at the "attachments" query for an explanation)
    if !item.properties_internal.is_empty() || !item.properties_custom.is_empty() {
        let property_sql: String = format!(
            "INSERT INTO item_properties (item_id,is_custom,name,value) VALUES (?,?,?,?){}",
            ", (?,?,?,?)".repeat(item.properties_internal.len() + item.properties_custom.len() - 1),
        );

        let mut property_insertion = sqlx::query(property_sql.as_str());
        for property in &item.properties_internal {
            property_insertion = property_insertion.bind(&item_id).bind(false).bind(&property.name).bind(&property.value);
        }

        for property in &item.properties_custom {
            property_insertion = property_insertion.bind(&item_id).bind(true).bind(&property.name).bind(&property.value);
        }

        property_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
    }

    // If we have attachments, store them in a separate table.
    if !item.attachments.is_empty() {
        // Create an sql query with the right amount of values
        // to fit all the attachments into a single query.
        // We only need to the string n -1 times, because we
        // already have the fist one in the formatting string.
        let attachment_sql: String = format!(
            "INSERT INTO item_attachments (item_id,name,url) VALUES (?,?,?){}",
            ", (?,?,?)".repeat(item.attachments.len() - 1)
        );

        // Insert all attachments into the sql query.
        let mut attachment_insertion = sqlx::query(attachment_sql.as_str());
        for attachment in &item.attachments {
            attachment_insertion = attachment_insertion.bind(&item_id).bind(attachment.0).bind(attachment.1);
        }

        // Execute the query and check for errors.
        attachment_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
    }

    // Finally commit the changes to make them permanent
    tx.commit().await.map_err(error::ErrorInternalServerError)?;
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "item_id" => item_id
    }))
}

#[rustfmt::skip]
#[actix_web::post("/item/{item_id}")]
async fn update_item(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest, item: web::Json<Item>) -> actix_web::Result<HttpResponse> {
    let item_id: u64 = get_param(&req, "item_id", "item id must be a number!")?;
    if item.id != item_id {
        return Err(error::ErrorBadRequest("the item ids don't match!"));
    }

    // ### FIXME ###
    // The following code is some real shit,
    // because it's basically just copy pasted code.
    // The code dose two things:
    // 1. delete the item
    // 2. put the modified item back in
    //
    // This is really inefficient and should be optimized in the future!
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // Delete the item
    let deletion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM items WHERE id = ?").bind(&item_id).execute(&mut tx).await;
    if deletion_query.map_err(error::ErrorInternalServerError)?.rows_affected() == 0 {
        return Err(error::ErrorNotFound("item not found!"));
    }

    // Add the new item back in
    let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO items (id,name,description,image,location_id,amount,last_edited,created) VALUES (?,?,?,?,?,?,?,?)")
            .bind(&item.id)
            .bind(&item.name)
            .bind(&item.description)
            .bind(&item.image)
            .bind(&item.location)
            .bind(&item.amount)
            .bind(&chrono::NaiveDateTime::from_timestamp(item.last_edited as i64, 0))
            .bind(&chrono::NaiveDateTime::from_timestamp(item.created as i64, 0))
            .execute(&mut tx)
            .await;

    if let Err(error) = insertion_query {
        return Err(match error {
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a item with this name!"),
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown location id!"),
            _ => error::ErrorInternalServerError(error),
        });
    }

    if !item.tags.is_empty() {
        let tag_sql: String = format!("INSERT INTO item_tags (item_id,tag_id) VALUES (?,?){}", ", (?,?)".repeat(item.tags.len() - 1));

        let mut tag_insertion = sqlx::query(tag_sql.as_str());
        for tag in &item.tags {
            tag_insertion = tag_insertion.bind(&item_id).bind(tag);
        }

        if let Err(error) = tag_insertion.execute(&mut tx).await {
            return Err(match error {
                //sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown tag id!"),
                _ => error::ErrorInternalServerError(error),
            });
        }
    }

    if !item.properties_internal.is_empty() || !item.properties_custom.is_empty() {
        let property_sql: String = format!(
            "INSERT INTO item_properties (item_id,is_custom,name,value) VALUES (?,?,?,?){}",
            ", (?,?,?,?)".repeat(item.properties_internal.len() + item.properties_custom.len() - 1),
        );

        let mut property_insertion = sqlx::query(property_sql.as_str());
        for property in &item.properties_internal {
            property_insertion = property_insertion.bind(&item_id).bind(false).bind(&property.name).bind(&property.value);
        }

        for property in &item.properties_custom {
            property_insertion = property_insertion.bind(&item_id).bind(true).bind(&property.name).bind(&property.value);
        }

        property_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
    }

    if !item.attachments.is_empty() {
        let attachment_sql: String = format!(
            "INSERT INTO item_attachments (item_id,name,url) VALUES (?,?,?){}",
            ", (?,?,?)".repeat(item.attachments.len() - 1)
        );

        let mut attachment_insertion = sqlx::query(attachment_sql.as_str());
        for attachment in &item.attachments {
            attachment_insertion = attachment_insertion.bind(&item_id).bind(attachment.0).bind(attachment.1);
        }

        attachment_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
    }

    tx.commit().await.map_err(error::ErrorInternalServerError)?;
    Ok(HttpResponse::Ok().finish())
}

#[actix_web::delete("/item/{item_id}")]
async fn delete_item(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<HttpResponse> {
    let item_id: u64 = get_param(&req, "item_id", "item id must be a number!")?;

    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM items WHERE id = ?").bind(&item_id).execute(pool.as_ref()).await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the item didn't even exist!
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("item not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::get("/tags")]
async fn get_tags(pool: web::Data<MySqlPool>, _user: AuthedUser) -> actix_web::Result<web::Json<Vec<Tag>>> {
    let tags = sqlx::query_as::<_, Tag>("SELECT * FROM tags").fetch_all(pool.as_ref()).await.unwrap();
    Ok(web::Json(tags))
}

#[actix_web::get("/tag/{tag_id}")]
async fn get_tag(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<web::Json<Tag>> {
    let tag_id: u64 = get_param(&req, "tag_id", "tag id must be a number!")?;

    // Query for the object and auto convert it.
    let query: Result<Tag, sqlx::Error> = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = ?").bind(tag_id).fetch_one(pool.as_ref()).await;

    // Check if the query was successful and return the tag,
    // if the tag could not be found, set the status code to 404.
    // Should a different kind of error occur, return an Internal Server Error (code: 500).
    match query {
        Ok(tag) => Ok(web::Json(tag)),
        Err(error) => Err(match error {
            sqlx::Error::RowNotFound => error::ErrorNotFound("tag not found!"),
            _ => error::ErrorInternalServerError(error),
        }),
    }
}

#[rustfmt::skip]
#[actix_web::put("/tag")]
async fn put_tag(pool: web::Data<MySqlPool>, _user: AuthedUser, tag: web::Json<Tag>) -> actix_web::Result<HttpResponse> {
    if tag.id != 0 {
        return Err(error::ErrorBadRequest("tag id must be 0!"));
    }

    // We need to make a transaction here because we want to make 2 queries that relate to each other.
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // First insert the object into the sql table...
    let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO tags (name,color,icon) VALUES (?,?,?)")
        .bind(&tag.name)
        .bind(&tag.color)
        .bind(&tag.icon)
        .execute(&mut tx)
        .await;

    // ...then make sure it didn't fail.
    if let Err(error) = insertion_query {
        return Err(match error {
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a tag with this name!"),
            _ => error::ErrorInternalServerError(error),
        });
    }

    // After that we need to get the autogenerated id from the table.
    let selection_query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT LAST_INSERT_ID()").fetch_one(&mut tx).await;

    // If we encounter an error then return status 500,
    // if not, extract the id from the query.
    let tag_id: u64 = selection_query.map_err(error::ErrorInternalServerError)?.try_get(0).unwrap();

    // Finally commit the changes to make them permanent
    tx.commit().await.map_err(error::ErrorInternalServerError)?;
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "tag_id" => tag_id
    }))
}

#[actix_web::post("/tag/{tag_id}")]
async fn update_tag(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest, tag: web::Json<Tag>) -> actix_web::Result<HttpResponse> {
    let tag_id: u64 = get_param(&req, "tag_id", "tag id must be a number!")?;
    if tag.id != tag_id {
        return Err(error::ErrorBadRequest("the tag ids don't match!"));
    }

    // Update the object in the sql table...
    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("UPDATE tags SET name = ?, color = ?, icon = ? WHERE id = ?")
        .bind(&tag.name)
        .bind(&tag.color)
        .bind(&tag.icon)
        .bind(&tag.id)
        .execute(pool.as_ref())
        .await;

    // ...then make sure it didn't fail.
    let result = match query {
        Ok(result) => result,
        Err(error) => {
            return Err(match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a tag with this name!"),
                _ => error::ErrorInternalServerError(error),
            })
        }
    };

    // If nothing was changed, the tag didn't even exist!
    if result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("tag not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::delete("/tag/{tag_id}")]
async fn delete_tag(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<HttpResponse> {
    let tag_id: u64 = get_param(&req, "tag_id", "tag id must be a number!")?;

    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM tags WHERE id = ?").bind(&tag_id).execute(pool.as_ref()).await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the tag didn't even exist!
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("tag not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::get("/databases")]
async fn get_databases(pool: web::Data<MySqlPool>, _user: AuthedUser) -> actix_web::Result<web::Json<Vec<Database>>> {
    let databases = sqlx::query_as::<_, Database>("SELECT * FROM item_databases").fetch_all(pool.as_ref()).await.unwrap();
    Ok(web::Json(databases))
}

#[actix_web::get("/database/{database_id}")]
async fn get_database(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<web::Json<Database>> {
    let database_id: u64 = get_param(&req, "database_id", "database id must be a number!")?;

    // Query for the object and auto convert it.
    let query: Result<Database, sqlx::Error> = sqlx::query_as::<_, Database>("SELECT * FROM item_databases WHERE id = ?")
        .bind(database_id)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the query was successful and return the database object,
    // if the database could not be found, set the status code to 404.
    // Should a different kind of error occur, return an Internal Server Error (code: 500).
    match query {
        Ok(database) => Ok(web::Json(database)),
        Err(error) => Err(match error {
            sqlx::Error::RowNotFound => error::ErrorNotFound("database not found!"),
            _ => error::ErrorInternalServerError(error),
        }),
    }
}

#[rustfmt::skip]
#[actix_web::put("/database")]
async fn put_database(pool: web::Data<MySqlPool>, _user: AuthedUser, database: web::Json<Database>) -> actix_web::Result<HttpResponse> {
    if database.id != 0 {
        return Err(error::ErrorBadRequest("database id must be 0!"));
    }

    // We need to make a transaction here because we want to make 2 queries that relate to each other.
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // First insert the object into the sql table...
    let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO item_databases (name) VALUES (?)")
        .bind(&database.name)
        .execute(&mut tx)
        .await;

    // ...then make sure it didn't fail.
    if let Err(error) = insertion_query {
        return Err(match error {
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a database with this name!"),
            _ => error::ErrorInternalServerError(error),
        });
    }

    // After that we need to get the autogenerated id from the table.
    let selection_query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT LAST_INSERT_ID()").fetch_one(&mut tx).await;

    // If we encounter an error then return status 500,
    // if not, extract the id from the query.
    let database_id: u64 = selection_query.map_err(error::ErrorInternalServerError)?.try_get(0).unwrap();

    // Finally commit the changes to make them permanent
    tx.commit().await.map_err(error::ErrorInternalServerError)?;
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "database_id" => database_id
    }))
}

#[actix_web::post("/database/{database_id}")]
async fn update_database(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest, database: web::Json<Database>) -> actix_web::Result<HttpResponse> {
    let database_id: u64 = get_param(&req, "database_id", "database id must be a number!")?;
    if database.id != database_id {
        return Err(error::ErrorBadRequest("the database ids don't match!"));
    }

    // Update the object in the sql table...
    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("UPDATE item_databases SET name = ? WHERE id = ?")
        .bind(&database.name)
        .bind(&database.id)
        .execute(pool.as_ref())
        .await;

    // ...then make sure it didn't fail.
    let result = match query {
        Ok(result) => result,
        Err(error) => {
            return Err(match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a database with this name!"),
                _ => error::ErrorInternalServerError(error),
            })
        }
    };

    // If nothing was changed, the database didn't even exist!
    if result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("database not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::delete("/database/{database_id}")]
async fn delete_database(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<HttpResponse> {
    let database_id: u64 = get_param(&req, "database_id", "database id must be a number!")?;

    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM item_databases WHERE id = ?").bind(&database_id).execute(pool.as_ref()).await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the database didn't even exist!
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("database not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::get("/locations")]
async fn get_locations(pool: web::Data<MySqlPool>, _user: AuthedUser) -> actix_web::Result<web::Json<Vec<Location>>> {
    let locations = sqlx::query_as::<_, Location>("SELECT * FROM locations").fetch_all(pool.as_ref()).await.unwrap();
    Ok(web::Json(locations))
}

#[actix_web::get("/location/{location_id}")]
async fn get_location(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<web::Json<Location>> {
    let location_id: u64 = get_param(&req, "location_id", "location id must be a number!")?;

    // Query for the object and auto convert it.
    let query: Result<Location, sqlx::Error> = sqlx::query_as::<_, Location>("SELECT * FROM locations WHERE id = ?")
        .bind(location_id)
        .fetch_one(pool.as_ref())
        .await;

    // Check if the query was successful and return the location,
    // if the location could not be found, set the status code to 404.
    // Should a different kind of error occur, return an Internal Server Error (code: 500).
    match query {
        Ok(location) => Ok(web::Json(location)),
        Err(error) => Err(match error {
            sqlx::Error::RowNotFound => error::ErrorNotFound("location not found!"),
            _ => error::ErrorInternalServerError(error),
        }),
    }
}

#[rustfmt::skip]
#[actix_web::put("/location")]
async fn put_location(pool: web::Data<MySqlPool>, _user: AuthedUser, location: web::Json<Location>) -> actix_web::Result<HttpResponse> {
    if location.id != 0 {
        return Err(error::ErrorBadRequest("location id must be 0!"));
    }

    // We need to make a transaction here because we want to make 2 queries that relate to each other.
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // First insert the object into the sql table...
    let insertion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("INSERT INTO locations (name,database_id) VALUES (?,?)")
        .bind(&location.name)
        .bind(&location.database)
        .execute(&mut tx)
        .await;

    // ...then make sure it didn't fail.
    if let Err(error) = insertion_query {
        return Err(match error {
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a location with this name!"),
            sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown database id!"),
            _ => error::ErrorInternalServerError(error),
        });
    }

    // After that we need to get the autogenerated id from the table.
    let selection_query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT LAST_INSERT_ID()").fetch_one(&mut tx).await;

    // If we encounter an error then return status 500,
    // if not, extract the id from the query.
    let location_id: u64 = selection_query.map_err(error::ErrorInternalServerError)?.try_get(0).unwrap();

    // Finally commit the changes to make them permanent
    tx.commit().await.map_err(error::ErrorInternalServerError)?;
    Ok(HttpResponse::Created().json::<HashMap<&str, u64>>(collection! {
        "location_id" => location_id
    }))
}

#[rustfmt::skip]
#[actix_web::post("/location/{location_id}")]
async fn update_location(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest, location: web::Json<Location>) -> actix_web::Result<HttpResponse> {
    let location_id: u64 = get_param(&req, "location_id", "location id must be a number!")?;
    if location.id != location_id {
        return Err(error::ErrorBadRequest("the location ids don't match!"));
    }

    // Update the object in the sql table...
    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("UPDATE locations SET name = ?, database_id = ? WHERE id = ?")
        .bind(&location.name)
        .bind(&location.database)
        .bind(&location.id)
        .execute(pool.as_ref())
        .await;

    // ...then make sure it didn't fail.
    let result = match query {
        Ok(result) => result,
        Err(error) => {
            return Err(match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a location with this name!"),
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => error::ErrorNotFound("unknown database id!"),
                _ => error::ErrorInternalServerError(error),
            })
        }
    };

    // If nothing was changed, the location didn't even exist!
    if result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("location not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[actix_web::delete("/location/{location_id}")]
async fn delete_location(pool: web::Data<MySqlPool>, _user: AuthedUser, req: HttpRequest) -> actix_web::Result<HttpResponse> {
    let location_id: u64 = get_param(&req, "location_id", "location id must be a number!")?;

    let query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM locations WHERE id = ?").bind(&location_id).execute(pool.as_ref()).await;

    // Get the query result or else return error 500.
    let query_result = query.map_err(error::ErrorInternalServerError)?;

    // If nothing was deleted, the location didn't even exist!
    if query_result.rows_affected() == 0 {
        return Err(error::ErrorNotFound("location not found!"));
    }

    Ok(HttpResponse::Ok().finish())
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerInfo {
    api_version: u32,
    server_version: String,
    os: Option<String>,
    os_version: Option<String>,
}

#[actix_web::get("/info")]
async fn get_system_info() -> actix_web::Result<web::Json<ServerInfo>> {
    let system_info = sysinfo::System::new();

    Ok(web::Json(ServerInfo {
        api_version: 1,
        server_version: String::from(option_env!("CARGO_PKG_VERSION").unwrap_or("unknown")),
        os: system_info.get_name(),
        os_version: system_info.get_os_version(),
    }))
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

fn sqlrow_to_basic_item(row: &sqlx::mysql::MySqlRow) -> Item {
    let last_edited: chrono::NaiveDateTime = row.get(6);
    let created: chrono::NaiveDateTime = row.get(7);

    Item {
        id: row.get(0),
        name: row.get(1),
        description: row.get(2),
        image: row.get(3),
        location: row.get(4),
        amount: row.get(5),
        last_edited: last_edited.timestamp() as u64,
        created: created.timestamp() as u64,
        tags: vec![],
        properties_custom: vec![],
        properties_internal: vec![],
        attachments: HashMap::new(),
    }
}
