def transform(event):
    if "fail" in event and event["fail"]:
        _ = event["missing_key"]
    event["processed"] = True
    return [event]
