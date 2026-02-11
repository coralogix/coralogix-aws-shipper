def transform(event):
    parsed = parse_json('{"nested": [1, 2, 3], "key": "value"}')
    return [{"original": event, "parsed": parsed}]
