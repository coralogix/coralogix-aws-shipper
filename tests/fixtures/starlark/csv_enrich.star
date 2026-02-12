# Enrich CSV-parsed dict events (header row as keys)
def transform(event):
    event["source"] = "csv"
    event["transformed"] = True
    return [event]
