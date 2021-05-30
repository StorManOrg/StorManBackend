use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub image: String,
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
    pub id: u64,
    pub name: String,
    pub value: String,
    pub display_type: Option<String>,
    pub min: Option<u64>,
    pub max: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tag {
    pub id: u64,
    pub name: String,
    pub color: u32,
    pub icon: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Location {
    pub id: u64,
    pub name: String,
    pub database: u64,
}

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone, Debug)]
pub struct Database {
    pub id: u64,
    pub name: String,
}
