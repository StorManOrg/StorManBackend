use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
}

/// If this struct is a parameter in an actix service,
/// it becomes a protected service
#[derive(Serialize, Deserialize, sqlx::FromRow, Debug)]
pub struct AuthedUser {
    pub session_id: String,
    pub user_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub image: Option<String>,
    pub location: u64,
    pub tags: Vec<u64>,
    pub amount: u64,
    pub properties_internal: Vec<Property>,
    pub properties_custom: Vec<Property>,
    pub attachments: HashMap<String, String>,
    pub last_edited: u64,
    pub created: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Property {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct Tag {
    pub id: u64,
    pub name: String,
    pub color: u32,
    pub icon: Option<u64>,
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct Location {
    pub id: u64,
    pub name: String,
    #[sqlx(rename = "database_id")]
    pub database: u64,
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct Database {
    pub id: u64,
    pub name: String,
}
