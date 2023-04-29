import json
import argparse

parser = argparse.ArgumentParser(description="Transform cache file to config file")
parser.add_argument('input', type=argparse.FileType('r'))
parser.add_argument('output', type=argparse.FileType('w'))

arg = parser.parse_args()

config = []
lines = arg.input.readlines()
for line in lines:
    try:
        cache_entry = json.loads(line)
        config.append({
            "patterns": [ cache_entry["pattern"] ],
            "reply": {
                "type": "PlainMessage",
                "data": cache_entry["reply"],
            }
        })
    except:
        print("error")

json.dump(config, arg.output, ensure_ascii=False)

