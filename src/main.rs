extern crate osmio;
extern crate byteorder;
extern crate bloom;

use std::fs;
use std::path::Path;
use osmio::{OSMReader, ObjId};
use osmio::pbf::PBFReader;
use std::env::args;
use std::collections::{HashSet, HashMap};
use std::io::{Read, Write, BufWriter, BufReader, Seek, SeekFrom};
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use bloom::BloomFilter;

mod sortedcollections;

fn latlon_to_3857(lat: f32, lon: f32) -> (f32, f32) {
    // There's a few metres difference if we do the calc in f32...
    let lon: f64 = lon as f64;
    let lat: f64 = lat as f64;
    let x: f64 = lon * 20037508.34 / 180.;
    let y: f64 = (((90. + lat) * ::std::f64::consts::PI / 360.).tan()).ln() / (::std::f64::consts::PI / 180.);
    let y: f64 = y * 20037508.34 / 180.;
    let y: f32 = y as f32;
    let x: f32 = x as f32;
    (x, y)
}

fn xy_to_tile(x: f32, y:f32, zoom: u8) -> (u32, u32) {
    if zoom == 0 { return (0, 0); }

    let max_x = 20037508.34;
    let max_y = 20037508.34;

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

struct NodeStoreWriter {
    max_node_id: u64,
    fp: BufWriter<fs::File>,
}

struct NodeStoreReader {
    fp: BufReader<fs::File>,
}

impl NodeStoreWriter {
    fn create(filename: &str) -> Self {
        let fp = BufWriter::new(fs::File::create(filename).unwrap());
        NodeStoreWriter{ max_node_id: 0, fp: fp }
    }


    fn set_node(&mut self, node_id: u64, lat: f32, lon: f32) {
        if self.max_node_id < node_id {
            //self.fp.set_len(node_id*8);
            self.fp.seek(SeekFrom::End(0)).unwrap();
            for _ in self.max_node_id..node_id {
                self.fp.write_f32::<BigEndian>(200f32).unwrap();
                self.fp.write_f32::<BigEndian>(200f32).unwrap();
            }
            self.max_node_id = node_id;
        }
        self.fp.seek(SeekFrom::Start(node_id*8)).unwrap();
        self.fp.write_f32::<BigEndian>(lat).unwrap();
        self.fp.write_f32::<BigEndian>(lon).unwrap();
    }
}

impl NodeStoreReader {
    fn open(filename: &str) -> Self {
        let fp = BufReader::new(fs::File::open(filename).unwrap());
        NodeStoreReader{ fp: fp }
    }

    fn get(&mut self, node_id: &u64) -> Option<(f32, f32)> {
        self.fp.seek(SeekFrom::Start(node_id*8)).unwrap();
        let lat = self.fp.read_f32::<BigEndian>().unwrap();
        let lon = self.fp.read_f32::<BigEndian>().unwrap();
        if lat == 200f32 || lon == 200f32 {
            None
        } else {
            Some((lat, lon))
        }
    }
}

fn extract_data(filename: &str) {
    let file = BufReader::new(fs::File::open(&Path::new(&filename)).unwrap());
    let mut way_reader = PBFReader::new(file);
    let obj_reader = way_reader.objects();

    let mut way_nodes_fp = BufWriter::new(fs::File::create("building-way-nodes").unwrap());

    let mut node_store = NodeStoreWriter::create("nodes");

    println!("Reading buildings");
    println!("    Reading Nodes & Buildings");
    let mut num_buildings = 0;
    let mut num_nodes = 0;
    for obj in obj_reader {
        match obj {
            osmio::OSMObj::Node(node) => {
                let node_id = node.id;
                if let (Some(lat), Some(lon)) = (node.lat, node.lon) {
                    node_store.set_node(node_id, lat, lon);
                }
            },
            osmio::OSMObj::Way(way) => {
                if way.tags.get("building").unwrap_or(&"no".to_string()) != "no" {
                    num_nodes += way.nodes.len();
                    for node in way.nodes {
                        way_nodes_fp.write_u64::<BigEndian>(node).unwrap();
                    }
                    way_nodes_fp.write_u64::<BigEndian>(0).unwrap();
                    num_buildings += 1;
                }
            },
            osmio::OSMObj::Relation(_) => {
                break;
            }
        }
    }
    println!("    There are {} buildings", num_buildings);
    drop(way_nodes_fp);
    drop(node_store);

}


fn get_next_way<R: Read>(way_nodes_fp: &mut R) -> Option<Vec<ObjId>> {
    let mut results = Vec::new();
    loop {
        match way_nodes_fp.read_u64::<BigEndian>() {
            Err(_) => {
                // EOF
                return None;
            },
            Ok(num) => {
                if num == 0 {
                    // end of this way
                    break
                } else {
                    results.push(num);
                }
            }
        }
    }

    Some(results)
}

fn calculate_angles(zoom_grouping: u8) -> HashMap<(u32, u32, i16), usize> {

    let mut results = HashMap::new();
    let mut way_nodes_fp = BufReader::new(fs::File::open("building-way-nodes").unwrap());

    let mut node_store = NodeStoreReader::open("nodes");

    println!("Calculating angles");
    loop {
        let building = get_next_way(&mut way_nodes_fp);
        if building.is_none() {
            break;
        }
        let building = building.unwrap();

        // last node is the first node for closed ways
        let first_corner = vec![building[building.len()-2], building[0], building[1]];
        let mut corners: Vec<_> = building.windows(3).collect();
        corners.push(&first_corner);
        for corner in corners {
            let (left_id, centre_id, right_id) = (corner[0], corner[1], corner[2]);
            let left = node_store.get(&left_id).unwrap();
            let centre = node_store.get(&centre_id).unwrap();
            let right = node_store.get(&right_id).unwrap();
            let this_angle = angle(centre.0, centre.1, left.0, left.1, right.0, right.1);
            let tile = xy_to_tile(centre.0, centre.1, zoom_grouping);
            *results.entry((tile.0, tile.1, this_angle)).or_insert(0) += 1;
        }

    }

    //fs::remove_file("building-way-nodes").ok();

    results

}

fn write_results(zoom_grouping: u8, first_results: HashMap<(u32, u32, i16), usize>, filename: &str) {
    let mut results = first_results;

    println!("All buildings calculated, writing results to {}", filename);
    let mut output_fp = BufWriter::new(fs::File::create(filename).unwrap());

    print!("Writing results for zoom ");
    output_fp.write(b"zoom,x,y,angle,count\n").unwrap();
    for this_zoom in (0..zoom_grouping+1).rev() {
        print!(" {}", this_zoom);
        std::io::stdout().flush().ok();
        let mut new_level = HashMap::new();
        for ((x, y, angle), count) in results {
            writeln!(output_fp, "{},{},{},{},{}", this_zoom, x, y, angle, count).unwrap();
            *new_level.entry((x/2, y/2, angle)).or_insert(0) += count;
        }
        results = new_level;
    }
    println!("")


}

fn main() {
    let input_filename = args().nth(1).unwrap();
    let output_filename = args().nth(2).unwrap();
    
    extract_data(&input_filename);

    let zoom_grouping = 18;

    let results = calculate_angles(zoom_grouping);

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

    #[test]
    fn test_transform() {
        use super::latlon_to_3857;
        assert_eq!(latlon_to_3857(29.91134, 11.92187), (1327136.50, 3492158.51));
    }

    #[test]
    fn test_xy_to_tile() {
        use super::{xy_to_tile, latlon_to_3857};
        let pos_3857 = latlon_to_3857(29.91134, 11.92187);
        let tiles = vec![ (0, 0), (1, 0), (2, 1), (4, 3), (8, 6), (17, 13), (34, 26), (68, 52), (136, 105), (272, 211), (545, 422), (1091, 845), (2183, 1691), (4367, 3382), (8734, 6764), (17469, 13528), (34938, 27057), (69876, 54114), (139753, 108228)];
        for (zoom, expected) in tiles.into_iter().enumerate() {
            assert_eq!(xy_to_tile(pos_3857.0, pos_3857.1, zoom as u8), expected);
        }
                        
    }
}


