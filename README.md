osm-building-angles

Compile:

    cargo build --release

Create CSV file of the results:

    ./target/release/osm-building-angles input-file.osm.pbf results.csv

Load the results into a SQLite database:

    ./create-sqlite-results.sh results.csv results.db

Now serve that sqlitefile as a tile source:

    python display/server.py 9000 ./results.db

It will listen on port 9000 and serve images as "/Z/X/Y.png". The file `./display/index.html` will display this on a map


