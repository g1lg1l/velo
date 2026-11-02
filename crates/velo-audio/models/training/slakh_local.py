"""Slakh's first parts, as downloaded (part0.bin, part1.bin...): read in order as one gzip'd tar,
keeping each track's mix, MIDI, metadata and stems, until the downloaded bytes end (or the
first part that isn't whole: the stream can't skip a gap)."""
import glob, os, re, sys, tarfile


class Parts:
    def __init__(self, folder):
        files = sorted(glob.glob(os.path.join(folder, "part*.bin")), key=lambda p: int(re.findall(r"\d+", os.path.basename(p))[0]))
        self.files = []
        for i, f in enumerate(files):
            self.files.append(f)
            if f != os.path.join(folder, f"part{i}.bin") or os.path.getsize(f) != 1 << 30:
                break
        self.i, self.fh = 0, open(self.files[0], "rb")

    def read(self, n=-1):
        out = b""
        while n < 0 or len(out) < n:
            chunk = self.fh.read(-1 if n < 0 else n - len(out))
            if chunk:
                out += chunk
                continue
            self.i += 1
            if self.i >= len(self.files):
                break
            self.fh.close()
            self.fh = open(self.files[self.i], "rb")
        return out


folder, out = sys.argv[1], sys.argv[2]
mixes = 0
try:
    with tarfile.open(fileobj=Parts(folder), mode="r|gz") as tar:
        for m in tar:
            parts = m.name.split("/")
            if not m.isfile() or ".." in parts or m.name.startswith("/"):
                continue
            if parts[-1] in ("mix.flac", "metadata.yaml") or m.name.endswith(".mid") or (len(parts) > 1 and parts[-2] == "stems"):
                tar.extract(m, out, filter="data")
                mixes += parts[-1] == "mix.flac"
except (EOFError, tarfile.ReadError, OSError) as e:
    print("stopped at the end of the downloaded parts:", type(e).__name__)
print(mixes, "mixes")
