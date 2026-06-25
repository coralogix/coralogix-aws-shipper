def transform(event):
    msg = event.get("msg", "")
    redacted = re_sub("password=\\S+", "password=***", msg)
    extracted = re_sub("user=(\\w+)", "uid:$1", msg)
    return [{"redacted": redacted, "extracted": extracted}]
