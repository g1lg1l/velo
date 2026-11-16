/*{
  "name": "AI Image",
  "category": "generator",
  "description": "Pictures a model on this computer makes from a prompt, played in turn like a Media List: in order, at random or back and forth, a new one every so many beats or seconds or on every hit (a Kick mapped onto Hit, which puts Clock on Hits: mapping.add with param hit and source kick), cut or crossfaded. Its prompt is its text (node.add's text, node.set_text); generate_image makes the pictures. It keeps the newest, as many as Pool says",
  "inputs": [{ "id": "s1", "type": "texture" }, { "id": "s2", "type": "texture" }, { "id": "s3", "type": "texture" }],
  "params": [
    { "id": "pool", "type": "int", "default": 8, "min": 1, "max": 64 },
    { "id": "refresh", "type": "int", "default": 0, "names": ["By hand", "Off the output"] },
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds", "Hits"] },
    { "id": "hit", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "movie_every", "type": "float", "default": 0.0, "min": 0.0, "max": 64.0 },
    { "id": "short_movie", "type": "int", "default": 0, "names": ["Moves on", "Repeats"] },
    { "id": "fade", "type": "float", "default": 0.25, "min": 0.0, "max": 1.0 },
    { "id": "load", "type": "int", "default": 1, "names": ["Streamed", "In memory"] },
    { "id": "_now", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "_fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// The app writes each picture to a file and adds it (node.add_media); from there it plays as a
// Media List does, through the same body (medialist-draw.wgsl). Pool and Refresh are the app's:
// how many it keeps, and whether it makes new ones each time its scene leaves the output.
