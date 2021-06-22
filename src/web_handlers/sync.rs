use std::collections::HashMap;

use actix_web::{error, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::{types::chrono, MySqlPool, Row};

use crate::models::{AuthedUser, Item, Property};
use crate::web_handlers::item::sqlrow_to_basic_item;

#[derive(Serialize, Deserialize, Debug)]
struct SyncPacket {
    delete: Vec<u64>,
    create: Vec<Item>,
}

#[derive(Serialize, Deserialize, Debug)]
struct SyncStatus {
    status: String,
}

#[actix_web::get("/sync")]
async fn sync(pool: web::Data<MySqlPool>, user: AuthedUser, req: web::Json<SyncPacket>) -> actix_web::Result<HttpResponse> {
    // +----------------------------------------+
    // |                 INFO                   |
    // | This function got a bit out of hand... |
    // |   But that's because some of it is     |
    // |         just copy and paste.           |
    // +----------------------------------------+

    // Create a transaction for all our queries.
    let mut tx = pool.as_ref().begin().await.map_err(error::ErrorInternalServerError)?;

    // Get the date when the user last synced their data.
    let last_sync: chrono::NaiveDateTime = sqlx::query("SELECT last_sync FROM sessions WHERE session_id = ?")
        .bind(&user.session_id)
        .fetch_one(pool.as_ref())
        .await
        .unwrap()
        .get(0);

    // This is the list where all the items that
    // are being returned to the user are stored.
    let mut create_items: Vec<Item> = Vec::new();

    // This is the list where all deleted items are
    // stored that will be returned to the user.
    // The list is iniciated by getting all the
    // item that got deleted while the user was absent.
    let mut delete_items: Vec<u64> = sqlx::query("SELECT item_id FROM item_deleted WHERE deleted > ?")
        .bind(&last_sync)
        .map(|row| row.get(0))
        .fetch_all(pool.as_ref())
        .await
        .unwrap();

    // Get and append all the new items that
    // were created while the user was offline.
    create_items.append(&mut {
        // (Look at the "get_items" function for an explanation)
        let mut items: HashMap<u64, Item> = sqlx::query("SELECT * FROM items WHERE created > ?")
            .bind(&last_sync)
            .fetch_all(pool.as_ref())
            .await
            .unwrap()
            .iter()
            .map(sqlrow_to_basic_item)
            .map(|item| (item.id, item))
            .collect();

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

                if let Some(item) = items.get_mut(&item_id) {
                    item.attachments.insert(name, url);
                }
            });

        items.into_iter().map(|(_, item)| item).collect()
    });

    // Get all the new items from the client
    // by cloning the list and filtering it with:
    // item created > last client sync
    let new_client_items: Vec<Item> = req.create.clone().into_iter().filter(|item| item.created > last_sync.timestamp() as u64).collect();
    new_client_items.iter().for_each(|item| delete_items.push(item.id));

    for mut item in new_client_items {
        // (Look at the "put_item" function for an explanation)
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

        if let Err(error) = insertion_query {
            return Err(match error {
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Duplicate entry") => error::ErrorConflict("there already is a item with this name!"),
                sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => {
                    error::ErrorNotFound("unknown location id!")
                }
                _ => error::ErrorInternalServerError(error),
            });
        }

        let selection_query: Result<sqlx::mysql::MySqlRow, sqlx::Error> = sqlx::query("SELECT LAST_INSERT_ID()").fetch_one(&mut tx).await;
        item.id = selection_query.map_err(error::ErrorInternalServerError)?.try_get(0).unwrap();

        if !item.tags.is_empty() {
            let tag_sql: String = format!("INSERT INTO item_tags (item_id,tag_id) VALUES (?,?){}", ", (?,?)".repeat(item.tags.len() - 1));

            let mut tag_insertion = sqlx::query(tag_sql.as_str());
            for tag in &item.tags {
                tag_insertion = tag_insertion.bind(&item.id).bind(tag);
            }

            if let Err(error) = tag_insertion.execute(&mut tx).await {
                return Err(match error {
                    sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => {
                        error::ErrorNotFound("unknown tag id!")
                    }
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
                property_insertion = property_insertion.bind(&item.id).bind(false).bind(&property.name).bind(&property.value);
            }

            for property in &item.properties_custom {
                property_insertion = property_insertion.bind(&item.id).bind(true).bind(&property.name).bind(&property.value);
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
                attachment_insertion = attachment_insertion.bind(&item.id).bind(attachment.0).bind(attachment.1);
            }

            attachment_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
        }

        create_items.push(item);
    }

    // Get all the items that got edited while the client
    // was absent. But we only want the once that the
    // client knows off. That meens they must have been
    // created before the client went offline.
    let edited_server_items: HashMap<u64, Item> = {
        // (Look at the "get_items" function for an explanation)
        let mut items: HashMap<u64, Item> = sqlx::query("SELECT * FROM items WHERE ? >= created AND ? < last_edited")
            .bind(&last_sync)
            .bind(&last_sync)
            .fetch_all(pool.as_ref())
            .await
            .unwrap()
            .iter()
            .map(sqlrow_to_basic_item)
            .map(|item| (item.id, item))
            .collect();

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

                if let Some(item) = items.get_mut(&item_id) {
                    item.attachments.insert(name, url);
                }
            });

        items
    };

    // Get all edited items from the client
    // by cloning the list and filtering it with:
    // item created >= last client sync && item edited > last client
    let edited_client_items: HashMap<u64, Item> = req
        .create
        .clone()
        .into_iter()
        .filter(|item| (last_sync.timestamp() as u64 >= item.created) && (item.last_edited > last_sync.timestamp() as u64))
        .map(|item| (item.id, item))
        .collect();

    // Create lists that contains all items that
    // are unique over both edited items lists.
    let edited_mergeable_server_items: Vec<Item> = edited_server_items
        .clone()
        .into_iter()
        .filter(|(item_id, _)| !edited_client_items.contains_key(item_id))
        .map(|(_, item)| item)
        .collect();

    let edited_mergeable_client_items: Vec<Item> = edited_client_items
        .clone()
        .into_iter()
        .filter(|(item_id, _)| !edited_server_items.contains_key(item_id))
        .map(|(_, item)| item)
        .collect();

    // If the amount of mergeable items is
    // the same as the amount of edited items,
    // we don't have a conflict :)
    //
    // In conclusion this means that:
    // +---------------------+          +-------------------------------+
    // |                     |          |                               |
    // |    The union of     |          |         The union of          |
    // | edited_server_items |          | edited_mergeable_server_items |
    // |         and         |  equals  |              and              |
    // | edited_client_items |          | edited_mergeable_client_items |
    // |                     |          |                               |
    // +---------------------+          +-------------------------------+
    if (edited_mergeable_server_items.len() + edited_mergeable_client_items.len()) == (edited_server_items.len() + edited_client_items.len()) {
        // Apply the changes the client requested.
        // We only need to do this for the items edited
        // by the client, because the sql database already
        // contains the changes made by the server.
        for item in &edited_mergeable_client_items {
            // (Look at the "update_item" function for an explanation)
            let deletion_query: Result<sqlx::mysql::MySqlQueryResult, sqlx::Error> = sqlx::query("DELETE FROM items WHERE id = ?").bind(&item.id).execute(&mut tx).await;
            if deletion_query.map_err(error::ErrorInternalServerError)?.rows_affected() == 0 {
                return Err(error::ErrorNotFound("item not found!"));
            }

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
                    sqlx::Error::Database(db_error) if db_error.message().starts_with("Cannot add or update a child row: a foreign key constraint fails") => {
                        error::ErrorNotFound("unknown location id!")
                    }
                    _ => error::ErrorInternalServerError(error),
                });
            }

            if !item.tags.is_empty() {
                let tag_sql: String = format!("INSERT INTO item_tags (item_id,tag_id) VALUES (?,?){}", ", (?,?)".repeat(item.tags.len() - 1));

                let mut tag_insertion = sqlx::query(tag_sql.as_str());
                for tag in &item.tags {
                    tag_insertion = tag_insertion.bind(&item.id).bind(tag);
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
                    property_insertion = property_insertion.bind(&item.id).bind(false).bind(&property.name).bind(&property.value);
                }

                for property in &item.properties_custom {
                    property_insertion = property_insertion.bind(&item.id).bind(true).bind(&property.name).bind(&property.value);
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
                    attachment_insertion = attachment_insertion.bind(&item.id).bind(attachment.0).bind(attachment.1);
                }

                attachment_insertion.execute(&mut tx).await.map_err(error::ErrorInternalServerError)?;
            }
        }

        // Add the mergeable items to the list of items
        // that will be deleted (renewed) by the client.
        delete_items.append(&mut edited_mergeable_server_items.iter().map(|item| item.id).collect());

        // And also add the items changed by the server
        // to the list of newly (renewed) created items.
        let mut edited_mergeable_server_items = edited_mergeable_server_items;
        create_items.append(&mut edited_mergeable_server_items);

        // Finally commit the changes to make them permanent
        tx.rollback().await.map_err(error::ErrorInternalServerError)?;
        //tx.commit().await.map_err(error::ErrorInternalServerError)?;
        Ok(HttpResponse::Ok().json(SyncPacket {
            delete: delete_items,
            create: create_items,
        }))
    } else {
        Err(error::ErrorConflict(""))
    }
}
