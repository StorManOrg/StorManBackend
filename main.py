from typing import List, Dict, Optional
from fastapi import FastAPI
from pydantic import BaseModel
import string
import random

app = FastAPI(
    title="StoRe API",
    description="Storage Reloaded Backend (written in Python w/ FastAPI)",
    version="0.1.0"
)

class Property(BaseModel):
    id: int
    name: str
    value: str
    display_type: Optional[str]
    min: Optional[int]
    max: Optional[int]

class Item(BaseModel):
    id: int
    name: str
    description: str
    image: str
    location: str
    tags: List[int]
    properties_custom: List[Property] = []
    properties_internal: List[Property] = []
    attachments: Dict = []
    last_edited: int
    created: int

@app.get("/")
def root():
    return {"version": app.version}

@app.get("/auth")
def login(username: str, password: str):
    return {"access_token": ''.join(random.choices(string.ascii_uppercase + string.digits, k=6))}


@app.get("/items/{item_id}")
def read_item(item_id: int, q: Optional[str] = None):
    return {"item_id": item_id, "q": q}

@app.put("/items/{item_id}")
def update_item(item_id: int, item: Item):
    return {"item_name": item.name, "item_id": item_id}