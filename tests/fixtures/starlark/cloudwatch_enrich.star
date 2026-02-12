# CloudWatch: event is the raw message string; wrap in dict to verify transform runs
def transform(event):
    if type(event) == "string":
        return [{"message": event, "starlark_enriched": True}]
    return [event]
