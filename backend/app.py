import os, sys
from contextlib import asynccontextmanager
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from dotenv import load_dotenv

load_dotenv()

@asynccontextmanager
async def lifespan(app: FastAPI):
    import idf
    idf.ensure_loaded()
    import learned_weights
    learned_weights.ensure_loaded()
    yield

app = FastAPI(title="Sounds Like You — Discovery", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

from chat_routes import router as chat_router
from taste_routes import router as taste_router
from similar_routes import router as similar_router
from multimodal_routes import router as multimodal_router
from weights_routes import router as weights_router
import data_loader

app.include_router(chat_router, prefix="/api")
app.include_router(taste_router, prefix="/api")
app.include_router(similar_router, prefix="/api")
app.include_router(multimodal_router, prefix="/api")
app.include_router(weights_router, prefix="/api")

@app.get("/api/health")
def health():
    return {"status": "ok", "tracks": len(data_loader.TRACKS), "users": len(data_loader.USERS)}

@app.get("/api/users")
def users():
    return sorted(
        [{"id": uid, "n": len(cids)} for uid, cids in data_loader.USERS.items()],
        key=lambda x: -x["n"]
    )[:30]

audio_dir = os.path.expanduser("~/mml-hackatune-26/harvester/corpus/audio")
if os.path.isdir(audio_dir):
    app.mount("/audio", StaticFiles(directory=audio_dir), name="audio")

static_dir = os.path.join(os.path.dirname(__file__), "static")
if os.path.isdir(static_dir):
    app.mount("/", StaticFiles(directory=static_dir, html=True), name="static")
