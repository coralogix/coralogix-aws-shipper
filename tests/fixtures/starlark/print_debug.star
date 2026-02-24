def transform(event):
    """Test that print() works for debugging."""
    print("Processing event:", event)
    if "debug" in event:
        print("Debug mode enabled")
    return [event]
