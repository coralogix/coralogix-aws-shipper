def transform(event):
    # Simulates a script that returns empty string for dropped/invalid records.
    # The transform layer does NOT filter these - the caller (process_batches) must.
    if event.get("drop") == True:
        return [""]
    return [event]
