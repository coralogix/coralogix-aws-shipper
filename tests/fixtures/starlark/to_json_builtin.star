def transform(event):
    json_str = to_json({"key": "value", "num": 42, "bool": True})
    return [{"serialized": json_str}]
