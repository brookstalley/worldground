#!/usr/bin/env python3
"""Generate an animated hero GIF for the README.

Creates a hex-grid animation showing weather patterns sweeping across terrain,
mimicking what the actual Worldground viewer displays.
"""

import math
import os
import random
from PIL import Image, ImageDraw, ImageFont

WIDTH, HEIGHT = 800, 300
FRAMES = 48
HEX_SIZE = 9  # radius (center to vertex)

# Pointy-top hex geometry
HEX_W = math.sqrt(3) * HEX_SIZE    # width (flat edge to flat edge)
HEX_H = 2 * HEX_SIZE               # height (vertex to vertex)
COL_STEP = HEX_W                    # horizontal spacing between columns
ROW_STEP = HEX_H * 0.75            # vertical spacing between rows

COLS = int(WIDTH / COL_STEP) + 3
ROWS = int(HEIGHT / ROW_STEP) + 3

# Terrain palette
BIOME_COLORS = {
    "deep_ocean": (12, 40, 90),
    "ocean": (20, 60, 120),
    "coast": (45, 105, 165),
    "plains": (115, 160, 55),
    "forest": (35, 100, 35),
    "boreal": (45, 75, 55),
    "mountain": (135, 125, 115),
    "desert": (195, 175, 115),
    "tundra": (175, 195, 205),
    "snow": (225, 235, 245),
}


def hex_corners(cx, cy, size):
    """Pointy-top hex corners (vertex at top and bottom)."""
    return [
        (cx + size * math.cos(math.radians(60 * i - 30)),
         cy + size * math.sin(math.radians(60 * i - 30)))
        for i in range(6)
    ]


def generate_terrain(seed=42):
    """Generate a terrain grid with coherent landmasses."""
    random.seed(seed)
    grid = {}

    # Elevation noise from scattered influence points
    points = [(random.uniform(0, COLS), random.uniform(0, ROWS)) for _ in range(14)]
    weights = [random.uniform(0.4, 1.4) for _ in points]

    for r in range(ROWS):
        for c in range(COLS):
            elev = 0.0
            for (px, py), w in zip(points, weights):
                d = math.sqrt((c - px) ** 2 + (r - py) ** 2)
                elev += math.exp(-d * 0.07) * w

            lat_factor = abs(r - ROWS / 2) / (ROWS / 2)

            if elev < 1.6:
                biome = "deep_ocean"
            elif elev < 2.1:
                biome = "ocean"
            elif elev < 2.5:
                biome = "coast"
            elif elev > 5.0:
                biome = "mountain" if lat_factor < 0.7 else "snow"
            elif lat_factor > 0.78:
                biome = "tundra" if elev < 3.5 else "snow"
            elif lat_factor > 0.55:
                biome = "boreal"
            elif elev > 3.8:
                biome = "forest"
            elif lat_factor < 0.2 and elev < 3.0:
                biome = "desert"
            else:
                biome = "plains"

            grid[(r, c)] = {"biome": biome, "elev": elev}
    return grid


def weather_color(base_color, frame, col, row, grid_data):
    """Apply weather overlay: clouds, rain, and seasonal temperature shifts."""
    r, g, b = base_color

    # Sweeping cloud band (moves right over time)
    cloud_center = (frame * 1.2) % (COLS + 20) - 10
    cloud_dist = abs(col - cloud_center)
    cloud_width = 5 + 2.5 * math.sin(row * 0.4)

    # Secondary cloud band
    cloud2_center = ((frame * 0.8) + COLS * 0.6) % (COLS + 20) - 10
    cloud2_dist = abs(col - cloud2_center)

    cloud_factor = max(0, 1 - cloud_dist / cloud_width)
    cloud_factor = max(cloud_factor, max(0, 1 - cloud2_dist / (cloud_width * 0.7)) * 0.6)

    biome = grid_data.get("biome", "ocean")
    rain = cloud_factor > 0.5 and biome not in ("ocean", "deep_ocean")

    # Seasonal color shift
    season_phase = (frame / FRAMES) * 2 * math.pi
    temp_shift = math.sin(season_phase) * 12

    # Cloud whitening
    r = int(min(255, r + cloud_factor * (225 - r) * 0.65))
    g = int(min(255, g + cloud_factor * (230 - g) * 0.65))
    b = int(min(255, b + cloud_factor * (240 - b) * 0.65))

    # Rain darkening
    if rain:
        r = int(r * 0.82)
        g = int(g * 0.85)
        b = int(min(255, b * 1.05 + 12))

    # Temperature tint
    r = int(max(0, min(255, r + temp_shift * 0.3)))
    b = int(max(0, min(255, b - temp_shift * 0.2)))

    return (r, g, b)


def draw_frame(terrain, frame):
    """Draw one frame of the animation."""
    img = Image.new("RGB", (WIDTH, HEIGHT), (8, 8, 20))
    draw = ImageDraw.Draw(img)

    for r in range(ROWS):
        for c in range(COLS):
            # Pointy-top hex center: odd rows offset right by half a column
            cx = c * COL_STEP + (COL_STEP * 0.5 if r % 2 else 0)
            cy = r * ROW_STEP

            if cx < -HEX_SIZE * 2 or cx > WIDTH + HEX_SIZE * 2:
                continue
            if cy < -HEX_SIZE * 2 or cy > HEIGHT + HEX_SIZE * 2:
                continue

            data = terrain.get((r, c), {"biome": "deep_ocean", "elev": 0})
            base = BIOME_COLORS.get(data["biome"], (100, 100, 100))
            color = weather_color(base, frame, c, r, data)

            corners = hex_corners(cx, cy, HEX_SIZE)
            draw.polygon(corners, fill=color, outline=None)

    # Title overlay with darkened band
    try:
        font = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 28)
        small = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", 14)
    except (OSError, IOError):
        try:
            font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 28)
            small = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 14)
        except (OSError, IOError):
            font = ImageFont.load_default()
            small = font

    # Darken band behind title
    for y in range(8, 58):
        for x in range(12, 440):
            if 0 <= x < WIDTH and 0 <= y < HEIGHT:
                pr, pg, pb = img.getpixel((x, y))
                img.putpixel((x, y), (pr // 3, pg // 3, max(0, pb // 3 + 15)))

    draw.text((18, 10), "worldground", fill=(240, 240, 255), font=font)
    draw.text((18, 38), "perpetual world simulation engine", fill=(155, 165, 195), font=small)

    return img


def main():
    print("Generating terrain...")
    terrain = generate_terrain(seed=1337)

    print(f"Rendering {FRAMES} frames ({COLS}x{ROWS} hex grid, size={HEX_SIZE})...")
    frames = []
    for i in range(FRAMES):
        frames.append(draw_frame(terrain, i))
        if (i + 1) % 12 == 0:
            print(f"  Frame {i + 1}/{FRAMES}")

    out = os.path.join(os.path.dirname(os.path.dirname(__file__)), "docs", "hero.gif")
    os.makedirs(os.path.dirname(out), exist_ok=True)

    print(f"Saving {out}...")
    frames[0].save(
        out,
        save_all=True,
        append_images=frames[1:],
        duration=120,
        loop=0,
    )
    print(f"Done! {os.path.getsize(out) / 1024:.0f} KB")


if __name__ == "__main__":
    main()
