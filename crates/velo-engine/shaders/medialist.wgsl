/*{
  "name": "Media List",
  "category": "generator",
  "description": "Pictures and movies in turn, streamed from disk or held in memory: in order, at random (every file once before any comes again, another way round each time) or back and forth, a new one every so many beats or seconds (movies for as long as their own) or on every hit (a Kick mapped onto Hit, which puts Clock on Hits), cut or crossfaded",
  "inputs": [{ "id": "s1", "type": "texture" }, { "id": "s2", "type": "texture" }, { "id": "s3", "type": "texture" }],
  "params": [
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds", "Hits"] },
    { "id": "hit", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "movie_every", "type": "float", "default": 0.0, "min": 0.0, "max": 64.0 },
    { "id": "short_movie", "type": "int", "default": 0, "names": ["Moves on", "Repeats"] },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "load", "type": "int", "default": 0, "names": ["Streamed", "In memory"] },
    { "id": "_now", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "_fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/
