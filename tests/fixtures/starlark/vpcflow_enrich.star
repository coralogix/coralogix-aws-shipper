# Enrich VPC Flow Log dicts (header row as keys)
def transform(event):
    event["starlark_enriched"] = True
    return [event]
