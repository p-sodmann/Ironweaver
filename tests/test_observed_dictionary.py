from ironweaver import ObservedDictionary


class Recorder:
    def __init__(self):
        self.calls = 0
        self.args = []

    def cb(self, node, key, value, old_value):
        self.calls += 1
        self.args.append((node, key, value, old_value))


def test_setitem_callback_on_change():
    rec = Recorder()
    d = ObservedDictionary(None, {"foo": [rec.cb]})
    d["foo"] = 1
    assert rec.calls == 1
    assert rec.args[0][1:] == ("foo", 1, None)

    d["foo"] = 1
    assert rec.calls == 1  # no new call when value unchanged

    d["foo"] = 2
    assert rec.calls == 2
    assert rec.args[1][1:] == ("foo", 2, 1)

