#!/usr/bin/env zsh

# Script to generate PNG icons from assets/logo.svg
# Uses rsvg-convert for high-quality SVG rendering and ImageMagick for sharpening

set -e

# Get the directory where this script is located
SCRIPT_DIR="${0:a:h}"

SVG_SOURCE="$SCRIPT_DIR/logo.svg"
OUTPUT_DIR="$SCRIPT_DIR"

# Check if required tools are installed
if ! command -v rsvg-convert &> /dev/null; then
    echo "Error: rsvg-convert not found. Install with: brew install librsvg"
    exit 1
fi

if ! command -v magick &> /dev/null; then
    echo "Error: ImageMagick not found. Install with: brew install imagemagick"
    exit 1
fi

if [ ! -f "$SVG_SOURCE" ]; then
    echo "Error: $SVG_SOURCE not found"
    exit 1
fi

# Function to generate PNG with appropriate sharpening
# Args: size, output_filename
generate_icon() {
    local size=$1
    local output="$OUTPUT_DIR/$2"
    
    echo "Generating $output (${size}x${size})..."
    
    # Render at 2x size for better quality, then downscale
    local render_size=$((size * 2))
    
    # For very small sizes (<=64px), apply sharpening
    if [ $size -le 64 ]; then
        # Render at higher resolution for better downscaling
        rsvg-convert -w $render_size -h $render_size "$SVG_SOURCE" | \
        magick - \
            -resize ${size}x${size} \
            -sharpen 0x1.0 \
            -quality 100 \
            "$output"
    elif [ $size -le 256 ]; then
        # Medium sizes: light sharpening
        rsvg-convert -w $render_size -h $render_size "$SVG_SOURCE" | \
        magick - \
            -resize ${size}x${size} \
            -sharpen 0x0.5 \
            -quality 100 \
            "$output"
    else
        # Large sizes: render directly without extra sharpening
        rsvg-convert -w $size -h $size "$SVG_SOURCE" -o "$output"
        
        # Apply minimal sharpening for crispness
        magick "$output" -sharpen 0x0.3 -quality 100 "$output"
    fi
}

echo "Starting icon generation from $SVG_SOURCE"
echo "============================================"

# Standard icon sizes (extracted from existing files)
generate_icon 16 "logo@16.png"
generate_icon 32 "logo@32.png"
generate_icon 64 "logo@64.png"
generate_icon 128 "logo@128.png"
generate_icon 256 "logo@256.png"
generate_icon 512 "logo@512.png"
generate_icon 1024 "logo@1024.png"
generate_icon 2048 "logo@2048.png"

# Fractional sizes (based on existing naming)
generate_icon 32 "logo@0.015625x.png"   # 32x32
generate_icon 64 "logo@0.03125x.png"    # 64x64
generate_icon 128 "logo@0.0625x.png"    # 128x128
generate_icon 205 "logo@0.1x.png"       # 205x205
generate_icon 266 "logo@0.13x.png"      # 266x266
generate_icon 512 "logo@0.25x.png"      # 512x512
generate_icon 1024 "logo@0.5x.png"      # 1024x1024

# Standard logo
generate_icon 128 "logo.png"            # Default logo size

# Full-size logo for splash screens (high-quality, no sharpening)
echo "Generating $OUTPUT_DIR/logo-full.png (1024x1024, high-quality)..."
rsvg-convert -w 2048 -h 2048 "$SVG_SOURCE" | \
magick - -resize 1024x1024 -quality 100 "$OUTPUT_DIR/logo-full.png"

echo "============================================"
echo "Icon generation complete!"
echo "All PNG files have been generated in $OUTPUT_DIR/"
