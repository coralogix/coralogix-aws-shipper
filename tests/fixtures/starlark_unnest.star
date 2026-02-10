def transform(event):
    keep = []
    if "logs" in event and type(event["logs"]) == "list":
        for log in event["logs"]:
            if "level" in log and log["level"] != "warn":
                keep.append(log)
        return keep
    return [event]
