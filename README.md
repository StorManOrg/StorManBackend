# Backend
WIP

## Usage
### Dependencies
* Python3
* FastAPI
* Uvicorn

For **Arch Linux** based distros, you can install the `python-fastapi` and the `uvicorn` package.

For **Debian** based distros, install `python3` and then run `pip install -r requirements.txt` inside the project folder.

### Running it
1. Clone the repo
2. Open a terminal and go to the project folder
3. Run `uvicorn main:app --reload` to start the server
3. Go to http://127.0.0.1:8000/docs (also try: http://127.0.0.1:8000/redoc)