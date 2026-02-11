def transform(event):
    event["processed"] = True
    event["source"] = "aws-shipper"
    return [event]
