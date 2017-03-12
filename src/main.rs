extern crate osmio;
//extern crate rusqlite;

use std::fs;
use std::path::Path;
use osmio::{OSMReader, ObjId};
use osmio::pbf::PBFReader;
use std::env::args;
use std::collections::{HashSet, HashMap};
//use rusqlite::Connection;
use std::io::Write;

mod sortedcollections;

fn latlon_to_3857(lat: f32, lon: f32) -> (f32, f32) {
    let x = lon * 20037508.34 / 180.;
    let y = (((90. + lat) * ::std::f32::consts::PI / 360.).tan()).ln() / (::std::f32::consts::PI / 180.);
    let y = y * 20037508.34 / 180.;
    (x, y)
}

fn xy_to_tile(x: f32, y:f32, zoom: u8) -> (u32, u32) {
    if zoom == 0 { return (0, 0); }

    let max_x = 20026376.39;
    let max_y = 20048966.10;

    let x_frac = (max_x + x) / (2. * max_x);
    let y_frac = (max_y - y) / (2. * max_y);

    let num_tiles = 2f32.powi(zoom as i32);

    let xtile = (num_tiles * x_frac).trunc() as u32;
    let ytile = (num_tiles * y_frac).trunc() as u32;

    (xtile, ytile)
}


fn angle(x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) -> i16 {
    let dist_12_sq = (x2-x1).powi(2) + (y2-y1).powi(2);
    let dist_13_sq = (x3-x1).powi(2) + (y3-y1).powi(2);
    let dist_23_sq = (x3-x2).powi(2) + (y3-y2).powi(2);
    // TODO don't sqrt and then square again
    // https://stackoverflow.com/questions/1211212/how-to-calculate-an-angle-from-three-points
    // and https://en.wikipedia.org/wiki/Law_of_cosines
    let angle = ( ( ( dist_12_sq + dist_13_sq - dist_23_sq ) / (4.*dist_12_sq*dist_13_sq).sqrt() ) ).acos();

    angle.to_degrees().round() as i16
}

fn read_file(filename: &str) -> (Vec<osmio::Way>, HashMap<ObjId, (f32, f32)>) {
    let file = fs::File::open(&Path::new(&filename)).unwrap();
    let mut way_reader = PBFReader::new(file);
    let way_reader = way_reader.ways();

    let mut nodes_needed: HashSet<ObjId> = HashSet::new();

    let mut building_ways = Vec::new();
    println!("Reading buildings");
    println!("    Reading ways");
    for way in way_reader {
        if way.tags.get("building").unwrap_or(&"no".to_string()) != "no" {
            nodes_needed.extend(way.nodes.iter());
            building_ways.push(way);
        }
    }
    println!("    There are {} buildings", building_ways.len());

    let file = fs::File::open(&Path::new(&filename)).unwrap();
    let mut node_reader = PBFReader::new(file);
    let node_reader = node_reader.nodes();

    let mut node_locations: HashMap<ObjId, (f32, f32)> = HashMap::new();
    
    println!("    Reading nodes");
    for node in node_reader {
        // Might be quicker to use binary search thing
        if let (Some(lat), Some(lon)) = (node.lat, node.lon) {
            if nodes_needed.contains(&node.id) {
                let (x, y) = latlon_to_3857(lat, lon);
                node_locations.insert(node.id, (x, y));
            }
        }
    }

    (building_ways, node_locations)
}

fn calculate_angles(zoom_grouping: u8, building_ways: &Vec<osmio::Way>, node_locations: &HashMap<ObjId, (f32, f32)>) -> HashMap<(u32, u32, i16), usize> {

    let mut results = HashMap::new();

    println!("Calculating angles");
    for building in building_ways {

        //println!("Looking at building {} which has {} points: {:?}", building.id, building.nodes.len(), building.nodes);

        // last node is the first node for closed ways
        let first_corner = vec![building.nodes[building.nodes.len()-2], building.nodes[0], building.nodes[1]];
        let mut corners: Vec<_> = building.nodes.windows(3).collect();
        corners.push(&first_corner);
        for corner in corners {
            let (left_id, centre_id, right_id) = (corner[0], corner[1], corner[2]);
            let left = node_locations.get(&left_id).unwrap();
            let centre = node_locations.get(&centre_id).unwrap();
            let right = node_locations.get(&right_id).unwrap();
            let this_angle = angle(centre.0, centre.1, left.0, left.1, right.0, right.1);
            let tile = xy_to_tile(centre.0, centre.1, zoom_grouping);
            *results.entry((tile.0, tile.1, this_angle)).or_insert(0) += 1;
        }

    }

    results

}

fn write_results(zoom_grouping: u8, first_results: HashMap<(u32, u32, i16), usize>, filename: &str) {
    let mut results = first_results;

    println!("All buildings calculated, writing results to {}", filename);
    let mut output_fp = fs::File::create(filename).unwrap();
    output_fp.write(b"zoom,x,y,angle,count\n").unwrap();
    for this_zoom in (1..zoom_grouping+1).rev() {
        println!("Calculating zoom {}", this_zoom);
        let mut new_level = HashMap::new();
        for ((x, y, angle), count) in results {
            writeln!(output_fp, "{},{},{},{},{}", this_zoom, x, y, angle, count).unwrap();
            *new_level.entry((x/2, y/2, angle)).or_insert(0) += count;
        }
        results = new_level;
    }


}

fn main() {
    let input_filename = args().nth(1).unwrap();
    let output_filename = args().nth(2).unwrap();
    
    let (building_ways, node_locations) = read_file(&input_filename);

    let zoom_grouping = 18;

    let results = calculate_angles(zoom_grouping, &building_ways, &node_locations);

    write_results(zoom_grouping, results, &output_filename);

    println!("Finished");

}

#[cfg(test)]
mod tests {
    #[test]
    fn test_angle() {
        use super::angle;
        assert_eq!(angle(0., 0., -1., 0., 1., 0.), 180);
        
    }

    //#[test]
    fn test_xy_to_tile() {
        use super::{xy_to_tile, latlon_to_3857};
        let pos_3857 = latlon_to_3857(29.91134, 11.92187);
        let tiles = vec![ (0, 0), (1, 0), (2, 1), (4, 3), (8, 6), (17, 13), (34, 26), (68, 52), (136, 105), (272, 211), (545, 422), (1091, 845), (2183, 1691), (4367, 3382), (8734, 6764)];
        for (zoom, expected) in tiles.into_iter().enumerate() {
            assert_eq!(xy_to_tile(pos_3857.0, pos_3857.1, zoom as u8), expected);
        }
                        
    }

}
