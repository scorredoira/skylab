#!/bin/sh
# Rebuilds this folder from the originals: downloads each material at 2K from Poly Haven or
# ambientCG (both CC0) into a cache folder and shrinks it to 1024 x 1024 with
# `crates/engine/engine/examples/prepare_textures.rs`. Needs curl, jq, unzip and cargo.
#
#   assets/textures/fetch.sh <cache_dir> [name...]      (run from the repository root)
#
# With names, only those materials are rebuilt. The table below is the single list of what is
# shipped; CREDITS.md repeats it with authors and links.
set -eu

cache=${1:?usage: fetch.sh <cache_dir> [name...]}
shift
out=$(dirname "$0")
agent="texture-fetch/1.0"
size=1024

# name | site (ph = polyhaven.com, acg = ambientcg.com) | id on that site
materials="
grass_short|acg|Grass004
grass_meadow|acg|Ground037
grass_dry|ph|grass_ground
soil_ploughed|ph|farm_soil
crop_stubble|ph|withered_grass
crop_green|ph|sparse_grass
forest_floor|ph|forrest_ground_03
rock_granite|ph|rock_04
rock_layered|acg|Rock023
scree|ph|rocks_ground_09
snow|acg|Snow010A
sand|ph|dense_sand
asphalt|ph|asphalt_04
concrete|ph|brushed_concrete
dirt_road|ph|dirt_floor
metal_painted|acg|Metal016
metal_corrugated|ph|corrugated_iron_02
plaster|ph|painted_plaster_wall
roof_clay|ph|clay_roof_tiles_02
roof_slate|ph|grey_roof_tiles_02
plastic|acg|Plastic010
leather|acg|Leather026
fabric|acg|Fabric036
carpet|acg|Carpet012
rubber|acg|Rubber004
bark_furrowed|ph|bark_brown_02
bark_pine|ph|pine_bark
bark_palm|ph|palm_bark
bark_smooth|ph|chinese_hackberry_bark
"

# Leaves, twigs and grass cut out of the textures of Poly Haven's plant models (CC0), for the
# cards trees are made of: name | model | its colour map | its opacity map | rectangle x0,y0,x1,y1 in
# the 1K originals. Each is written as an RGBA PNG at half that size.
cutouts="
foliage_leaves|island_tree_01|leaves_diff|leaves_alpha|0,0,1024,1024
foliage_spruce|fir_tree_01|twig_diff|twig_alpha|180,30,1000,870
foliage_pine_1|pine_tree_01|twig_diff|twig_alpha|30,40,230,455
foliage_pine_2|pine_tree_01|twig_diff|twig_alpha|660,345,965,535
foliage_grass|grass_medium_01|Diffuse|Alpha|0,770,1024,1024
"

get() { # url, file
    [ -s "$2" ] || curl -sSL --fail -A "$agent" -o "$2" "$1"
}

for row in $materials; do
    name=${row%%|*}
    rest=${row#*|}
    site=${rest%%|*}
    id=${rest#*|}
    if [ $# -gt 0 ]; then
        case " $* " in *" $name "*) ;; *) continue ;; esac
    fi
    dir="$cache/$id"
    mkdir -p "$dir"
    maps=""
    if [ "$site" = ph ]; then
        get "https://api.polyhaven.com/files/$id" "$dir/files.json"
        # Colour, roughness and occlusion as JPEG, the normal map as PNG: it is the one that
        # suffers from compression blocks.
        for pair in color:Diffuse:jpg normal:nor_gl:png rough:Rough:jpg ao:AO:jpg; do
            kind=${pair%%:*}
            key=${pair#*:}
            ext=${key#*:}
            key=${key%%:*}
            url=$(jq -r --arg k "$key" --arg e "$ext" '.[$k]["2k"][$e].url // empty' "$dir/files.json")
            [ -n "$url" ] || continue
            get "$url" "$dir/$kind.$ext"
            maps="$maps $kind=$dir/$kind.$ext"
        done
    else
        get "https://ambientcg.com/get?file=${id}_2K-PNG.zip" "$dir/$id.zip"
        unzip -q -o -j "$dir/$id.zip" -d "$dir"
        for pair in color:Color normal:NormalGL rough:Roughness ao:AmbientOcclusion; do
            file="$dir/${id}_2K-PNG_${pair#*:}.png"
            [ -f "$file" ] && maps="$maps ${pair%%:*}=$file"
        done
    fi
    side=$size
    case $name in
        # The bark of the trees: colour and relief only, the shaders read nothing else of it, at
        # 512: a trunk is seldom more than a few hundred pixels tall.
        bark_*)
            maps=$(printf "%s\n" $maps | grep -E "^(color|normal)=" | tr "\n" " ")
            side=512
            ;;
    esac
    echo "$name  <-  $site/$id"
    # shellcheck disable=SC2086
    cargo run -q -p engine --example prepare_textures -- material "$out" "$name" "$side" $maps
done

for row in $cutouts; do
    IFS='|' read -r name id diffuse alpha rect <<EOF2
$row
EOF2
    if [ $# -gt 0 ]; then
        case " $* " in *" $name "*) ;; *) continue ;; esac
    fi
    dir="$cache/$id"
    mkdir -p "$dir"
    get "https://api.polyhaven.com/files/$id" "$dir/files.json"
    for key in "$diffuse" "$alpha"; do
        url=$(jq -r --arg k "$key" '.[$k]["1k"].png.url // empty' "$dir/files.json")
        get "$url" "$dir/$key.png"
    done
    echo "$name  <-  ph/$id $diffuse"
    cargo run -q -p engine --example prepare_textures -- cutout "$out/$name.png" 0.5 "$dir/$diffuse.png" "$dir/$alpha.png" "$rect"
done
