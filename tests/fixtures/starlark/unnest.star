def transform(event):
    if "logs" in event and type(event["logs"]) == "list":
        return event["logs"]
    return [event]
