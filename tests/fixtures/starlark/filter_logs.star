def transform(event):
    if event.get("level") == "DEBUG":
        return []
    return [event]
