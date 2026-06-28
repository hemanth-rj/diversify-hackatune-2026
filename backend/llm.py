"""
Gemini integration module for music discovery.

Functions:
  parse_search_intent(messages) -> {query, metadataFilter, summary}
  explain_track(name, artist, match_breakdown, auto_description) -> str
  describe_for_music(text, image_b64, mime) -> {query, metadataFilter, summary}
"""

import base64
import json
import os
import re

from google import genai
from google.genai import types

_client = None


def _get_client() -> genai.Client:
    global _client
    if _client is None:
        _client = genai.Client(api_key=os.environ["GEMINI_API_KEY"])
    return _client


_MODEL = "gemini-2.5-flash"


def _extract_json(text: str | None) -> dict:
    """Pull the first JSON object out of a Gemini response regardless of markdown wrapping."""
    if not text:
        raise ValueError("Empty response")
    text = text.strip()
    # Strip ```json ... ``` or ``` ... ``` fences
    if text.startswith("```"):
        parts = text.split("```")
        text = parts[1] if len(parts) >= 2 else text
        if text.startswith("json"):
            text = text[4:]
        text = text.strip()
    # Regex fallback: find the outermost {...}
    m = re.search(r'\{.*\}', text, re.DOTALL)
    if m:
        text = m.group()
    return json.loads(text)

# ---------------------------------------------------------------------------
# Condensed tag vocabulary (sourced from guides/tag_vocabularies.md)
# ---------------------------------------------------------------------------

_TAG_VOCAB = """\
## Tag Vocabularies (Cyanite)

MoodSimpleV2.tags (13):
  aggressive, calm, chill, dark, energetic, epic, happy, romantic, sad, scary, sexy, ethereal, uplifting

MoodAdvancedV2.tags (selected):
  adventurous, anthemic, anxious, bittersweet, bright, celebratory, cheerful, cold, creepy, dangerous,
  delicate, dreamy, eerie, emotional, euphoric, fearful, feelGood, gloomy, graceful, hopeful, inspirational,
  intense, intimate, introspective, joyous, laidBack, lonely, loving, majestic, melancholic, menacing,
  mysterious, nostalgic, ominous, passionate, peaceful, playful, powerful, quiet, reflective, relaxed,
  romantic, sad, sentimental, serene, soaring, solemn, soothing, spiritual, spooky, suspenseful, tender,
  tense, thoughtful, tranquil, triumphant, upbeat, victorious, warm, whimsical

MainGenreV2.tags (23):
  african, ambient, middleEastern, asian, blues, childrenJingle, classical, electronic,
  folkCountry, funkSoul, indian, jazz, latin, metal, pop, rapHipHop, reggae, rnb, rock,
  singerSongwriter, sound, soundtrack, spokenWord

SubgenreV2.tags (selected):
  abstractIdm, breakbeatDnb, deepHouse, electro, house, minimal, synthPop, techHouse, techno, trance,
  bluesRock, folkRock, hardRock, indieAlternative, psychProgRock, punk, rockAndRoll, popSoftRock,
  contemporaryRnb, gangsta, jazzyHipHop, popRap, trap, blackMetal, deathMetal, heavyMetal, metalcore,
  disco, funk, gospel, neoSoul, soul, bigBandSwing, bebop, contemporaryJazz, smoothJazz, country, folk

InstrumentsV2.tags (47):
  accordion, acousticGuitar, africanPercussion, asianFlute, asianStrings, banjo, bass, bassGuitar,
  bells, bongoConga, brass, celeste, cello, churchOrgan, clarinet, doubleBass, drumKit, electricGuitar,
  electricOrgan, electricPiano, electronicDrums, flute, horn, glockenspiel, harp, harpsichord, luteOud,
  mandolin, marimba, oboe, percussion, piano, pizzicato, saxophone, sitar, steelDrums, strings, synth,
  tabla, taiko, trumpet, tuba, ukulele, vibraphone, violin, whistling, woodwinds

TempoV1.tags (5): slow, mediumSlow, medium, mediumFast, fast

CharacterV2.tags (16):
  bold, cool, epic, ethereal, heroic, luxurious, magical, mysterious, playful, powerful, retro,
  sophisticated, sparkling, sparse, unpolished, warm

VocalsV2.tags: female, male, instrumental
VocalStyleV1.tags (selected): femaleChoir, maleChoir, femaleForegroundVocals, maleForegroundVocals,
  instrumental, syntheticForegroundVocals

MovementV2.tags (10): bouncing, driving, flowing, groovy, nonrhythmic, pulsing, robotic, running, steady, stomping
"""

# ---------------------------------------------------------------------------
# System prompts
# ---------------------------------------------------------------------------

_SEARCH_SYSTEM = (
    "You are a music search assistant for a catalog of 357,000 tracks analyzed by Cyanite's audio AI.\n\n"
    "Given a conversation about a desired musical vibe, extract a Cyanite search query and optional filters.\n\n"
    "Return ONLY valid JSON with no markdown:\n"
    "{\n"
    '  "query": "natural language description of the sound",\n'
    '  "metadataFilter": {},\n'
    '  "summary": "human-readable filter summary (e.g. \'Mood: dark, energetic · Instruments: piano\')"\n'
    "}\n\n"
    "Available metadataFilter keys (MongoDB operators: $gte $lte $eq $ne $in $nin $and $or):\n"
    "- BpmV2.tag: numeric BPM e.g. {\"$gte\": 120, \"$lte\": 140}\n"
    "- TempoV1.tag: use $eq with one of: slow | mediumSlow | medium | mediumFast | fast\n"
    "- MoodSimpleV2.scores.<mood>: 0-1 score threshold e.g. {\"$gte\": 0.6}\n"
    "  moods: aggressive, calm, chill, dark, energetic, epic, happy, romantic, sad, scary, sexy, ethereal, uplifting\n"
    "- MoodSimpleV2.tags: use $in/$nin e.g. {\"$in\": [\"dark\",\"energetic\"]}\n"
    "- MainGenreV2.tags: $in/$nin — see vocabulary below\n"
    "- InstrumentsV2.tags: $in/$nin — see vocabulary below\n"
    "- VocalsV2.tags: $in/$nin e.g. {\"$in\": [\"instrumental\"]}\n"
    "- CharacterV2.tags: $in/$nin e.g. {\"$in\": [\"epic\",\"heroic\"]}\n\n"
    "Steering rules:\n"
    '- "more energetic" → add MoodSimpleV2.scores.energetic: {"$gte": 0.6}\n'
    '- "add piano" → add InstrumentsV2.tags: {"$in": ["piano"]}\n'
    '- "less dark" → add MoodSimpleV2.scores.dark: {"$lte": 0.4}\n'
    '- "faster tempo" → add TempoV1.tag: {"$eq": "fast"}\n'
    '- "more cinematic" → append "cinematic" to query, add MainGenreV2.tags: {"$in": ["soundtrack"]}\n'
    '- "no vocals / instrumental" → add VocalsV2.tags: {"$in": ["instrumental"]}\n\n'
    + _TAG_VOCAB
)

_MULTIMODAL_SYSTEM = (
    "You translate creative input into music search parameters for Cyanite's audio AI catalog.\n\n"
    "Return ONLY valid JSON with no markdown:\n"
    "{\n"
    '  "query": "natural language description of the ideal music sound",\n'
    '  "metadataFilter": {},\n'
    '  "summary": "what you inferred: mood · genre · tempo · instruments"\n'
    "}\n\n"
    "For images: describe the atmosphere, emotion, setting → translate to audio terms.\n\n"
    "IMPORTANT — metadataFilter keys are CASE-SENSITIVE and use MongoDB operators ONLY.\n"
    "Allowed keys (use EXACT spelling):\n"
    '- BpmV2.tag: numeric e.g. {"$gte": 120, "$lte": 140}\n'
    '- TempoV1.tag: {"$eq": "slow"} — values: slow | mediumSlow | medium | mediumFast | fast\n'
    '- MoodSimpleV2.tags: {"$in": ["dark","energetic"]} — see vocabulary\n'
    '- MainGenreV2.tags: {"$in": ["soundtrack","ambient"]}\n'
    '- InstrumentsV2.tags: {"$in": ["piano","strings"]}\n'
    '- VocalsV2.tags: {"$in": ["instrumental"]}\n'
    '- CharacterV2.tags: {"$in": ["mysterious","sparse"]}\n'
    "Do NOT use: moodAdvancedV2, movementV2, subgenreV2, or any other field.\n"
    "Do NOT use bare arrays — always wrap with {\"$in\": [...]}.\n\n"
    + _TAG_VOCAB
)

# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def parse_search_intent(messages: list[dict]) -> dict:
    """Translate conversation history into {query, metadataFilter, summary}."""
    client = _get_client()
    try:
        # Convert Anthropic-style messages to Gemini contents
        history = []
        for m in messages[:-1]:
            role = "user" if m["role"] == "user" else "model"
            history.append(types.Content(role=role, parts=[types.Part(text=m["content"])]))

        last = messages[-1]["content"] if messages else "music"
        if isinstance(last, list):
            last = next(
                (block.get("text", "") for block in last if block.get("type") == "text"),
                "music",
            )

        resp = client.models.generate_content(
            model=_MODEL,
            config=types.GenerateContentConfig(
                system_instruction=_SEARCH_SYSTEM,
                max_output_tokens=512,
                temperature=0.2,
                thinking_config=types.ThinkingConfig(thinking_budget=0),
            ),
            contents=history + [types.Content(role="user", parts=[types.Part(text=last)])],
        )
        result = _extract_json(resp.text)
        if not isinstance(result, dict) or "query" not in result:
            raise ValueError("Missing 'query' key in LLM response")
        result.setdefault("metadataFilter", {})
        result.setdefault("summary", "")
        return result
    except Exception as e:
        print(f"[llm] parse_search_intent failed: {e}")
        last_text = next(
            (m["content"] for m in reversed(messages) if m.get("role") == "user"),
            "music",
        )
        if isinstance(last_text, list):
            last_text = next(
                (block.get("text", "") for block in last_text if block.get("type") == "text"),
                "music",
            )
        return {"query": last_text, "metadataFilter": {}, "summary": ""}


def explain_track(
    name: str,
    artist: str,
    match_breakdown: list[dict],
    auto_description: str,
) -> str:
    """Return a one-sentence explanation of why this track matches the search."""
    if not match_breakdown:
        return "Acoustically similar to your search."

    top = match_breakdown[:3]
    labels = [d.get("dimension", "").split(".")[-1] for d in top]

    try:
        client = _get_client()
        prompt = (
            f"Track: '{name}' by {artist}.\n"
            f"Auto-description: {auto_description or '(none)'}\n"
            f"Top matching audio dimensions: {', '.join(labels)}\n"
            f"Write ONE sentence (max 20 words) explaining why this track matches "
            f"the search, grounded in these audio characteristics. Do not start with 'This track'."
        )
        resp = client.models.generate_content(
            model=_MODEL,
            config=types.GenerateContentConfig(
                max_output_tokens=60,
                temperature=0.3,
                thinking_config=types.ThinkingConfig(thinking_budget=0),
            ),
            contents=prompt,
        )
        text = resp.text
        if not text:
            raise ValueError("Empty response from model")
        return text.strip()
    except Exception as e:
        print(f"[llm] explain_track failed: {e}")
        return f"Shares {', '.join(labels[:2])} characteristics."


def describe_image_for_music(base64_image: str, mime_type: str) -> dict:
    """Convert an image to {query, metadataFilter, summary} for music search."""
    return describe_for_music(image_b64=base64_image, mime=mime_type)


def describe_brief_for_music(text: str) -> dict:
    """Convert a text brief to {query, metadataFilter, summary} for music search."""
    return describe_for_music(text=text)


def describe_for_music(
    text: str | None = None,
    image_b64: str | None = None,
    mime: str | None = "image/jpeg",
) -> dict:
    """Convert image or text brief → {query, metadataFilter, summary}."""
    client = _get_client()

    parts: list[types.Part] = []
    if image_b64:
        parts.append(types.Part(
            inline_data=types.Blob(
                mime_type=mime or "image/jpeg",
                data=base64.b64decode(image_b64),
            )
        ))
        parts.append(types.Part(text="Describe what music would fit this image."))
        if text:
            parts.append(types.Part(text=f"Additional context: {text}"))
    else:
        parts.append(types.Part(text=f"Find music for this brief:\n\n{text}"))

    try:
        resp = client.models.generate_content(
            model=_MODEL,
            config=types.GenerateContentConfig(
                system_instruction=_MULTIMODAL_SYSTEM,
                max_output_tokens=1024,
                temperature=0.2,
                thinking_config=types.ThinkingConfig(thinking_budget=0),
            ),
            contents=[types.Content(role="user", parts=parts)],
        )
        result = _extract_json(resp.text)
        if not isinstance(result, dict) or "query" not in result:
            raise ValueError("Missing 'query' key in LLM response")
        result.setdefault("metadataFilter", {})
        result.setdefault("summary", "")
        return result
    except Exception as e:
        print(f"[llm] describe_for_music failed: {e}")
        fallback = text or "cinematic atmospheric music"
        return {"query": fallback, "metadataFilter": {}, "summary": fallback}
