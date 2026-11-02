/*{
  "name": "Text",
  "category": "generator",
  "description": "Words set in a font on clear: spacing, lines, outline, shadow and a box behind; reveal types them in",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "font", "type": "int", "default": 0, "names": ["Inter", "Mono", "File"] },
    { "id": "size", "type": "float", "default": 0.15, "min": 0.01, "max": 1.0 },
    { "id": "align", "type": "int", "default": 1, "names": ["Left", "Center", "Right"] },
    { "id": "tracking", "type": "float", "default": 0.0, "min": -0.1, "max": 1.0 },
    { "id": "line_height", "type": "float", "default": 1.0, "min": 0.5, "max": 3.0 },
    { "id": "wrap", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "weight", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "slant", "type": "float", "default": 0.0, "min": -0.5, "max": 0.5 },
    { "id": "uppercase", "type": "bool", "default": false },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "outline", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "outline_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "shadow", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "shadow_offset", "type": "vec2", "default": [0.04, 0.06], "min": -0.5, "max": 0.5 },
    { "id": "shadow_blur", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "shadow_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "box", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "box_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "box_padding", "type": "float", "default": 0.2, "min": 0.0, "max": 1.0 },
    { "id": "box_radius", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "reveal", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 }
  ]
}*/

// The drawing is text-draw.wgsl's, shared with the Text List.

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    shown = reveal * setup().z;
    return draw(uv);
}
