import sys
from BaseHTTPServer import BaseHTTPRequestHandler,HTTPServer
from PIL import Image, ImageDraw, ImageFont
import sqlite3
import cStringIO
from matplotlib import pyplot as plt
import math


fnt = ImageFont.truetype('/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf', 10)


class ImagesServer(BaseHTTPRequestHandler):
    def do_GET(self):
        try:
            path = self.path
            _, zoom, x, yext = path.split("/")
            y, ext = yext.split(".")
            zoom = int(zoom)
            x = int(x)
            y = int(y)
            self.send_response(200)
            self.send_header('Content-type','image/png')
            self.end_headers()
            # Send the html message
            im = Image.new('RGBA', (256, 256), '#ffffff00')

            d = ImageDraw.Draw(im)
            self.db_cursor.execute("select sum(count) from angles where zoom = ? and x = ? and y = ?", (zoom, x, y))
            num_buildings = self.db_cursor.fetchone()[0]

            if num_buildings > 0:
                d.rectangle(((0,0), (255, 255)), outline="#0004", fill="#0000")
                d.text((2,9), "{}/{}/{} Total: {:,}".format(zoom, x, y, num_buildings), font=fnt, fill='black')

                self.db_cursor.execute("select angle, count from angles where zoom = ? and x = ? and y = ? order by angle", (zoom, x, y))
                building_angles = self.db_cursor.fetchall()
                angles = [math.radians(x[0]) for x in building_angles]
                count = [x[1] for x in building_angles]
                
                plt.clf()
                plt.subplot(111, projection='polar')
                if num_buildings > 0:
                    plt.plot(angles, count)

                buffer = cStringIO.StringIO()
                plt.savefig(buffer, dpi=45, transparent=True, format='png')
                buffer.seek(0)
                graph = Image.open(buffer)

                im.paste(graph, (1, 25))
                buffer.close()

            im.save(self.rfile, "png")
        except Exception as ex:
            print "Error ", repr(ex)
            self.send_response(404)
            self.end_headers()
            self.rfile.write("Not found")



if __name__ == '__main__':
    port_number = int(sys.argv[1])
    db_filename = sys.argv[2]
    db_connection = sqlite3.connect(db_filename)
    db_cursor = db_connection.cursor()

    try:
        images_server = ImagesServer
        images_server.db_cursor = db_cursor
	server = HTTPServer(('', port_number), ImagesServer)
	print 'Started httpserver on port ' , port_number
	server.serve_forever()
    except KeyboardInterrupt:
	print '^C received, shutting down the web server'
	server.socket.close()
	
