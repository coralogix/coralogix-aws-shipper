def is_important(event):
    return event.get("severity", "INFO") in ["ERROR", "CRITICAL"]

def transform(event):
    if is_important(event):
        event["flagged"] = True
    return [event]
