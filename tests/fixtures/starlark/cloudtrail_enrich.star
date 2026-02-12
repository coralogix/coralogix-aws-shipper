# Enrich CloudTrail record dicts
def transform(event):
    event["starlark_enriched"] = True
    return [event]
