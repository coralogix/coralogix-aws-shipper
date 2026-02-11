def transform(event):
    if "logs" in event:
        return event["logs"]
    return [event]
