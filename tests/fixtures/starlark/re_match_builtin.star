def transform(event):
    msg = event.get("msg", "")
    return [{"matched": re_match("error", msg), "unmatched": re_match("^nomatch$", msg)}]
