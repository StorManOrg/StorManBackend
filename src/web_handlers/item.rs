use std::collections::HashMap;

use actix_web::{error, web, HttpRequest, HttpResponse};
use sqlx::{types::chrono, MySqlPool, Row};

use crate::collection;
use crate::models::{AuthedUser, Item, Property};
use crate::web_handlers::get_param;

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

pub(crate) fn sqlrow_to_basic_item(row: &sqlx::mysql::MySqlRow) -> Item {
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
