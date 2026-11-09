"""Driver: walk Config-0-ResolvedFeatures of one part (dump dir) and report."""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from walker import Archive, Fail, hexdump
import classes  # registers readers
from scan import scan

def config0_classes(c0):
    ev, cls, n = scan(c0, 0)
    return cls, n

def walk(dirn, verbose=False, base=None):
    rf = open(os.path.join(dirn, 'Config-0-ResolvedFeatures'), 'rb').read()
    c0 = open(os.path.join(dirn, 'Config-0'), 'rb').read()
    cls0, n0 = config0_classes(c0)
    bases = [base] if base is not None else sorted(range(max(1, n0 - 60), n0 + 60), key=lambda b: abs(b - n0))
    best = None
    for b in bases:
        a = Archive(rf, b, cls0)
        try:
            top = classes.read_top(a)
            return a, top, None, b
        except Fail as e:
            if best is None or a.p > best[0].p: best = (a, e, b)
        except Exception as e:
            if best is None or a.p > best[0].p: best = (a, e, b)
    a, e, b = best
    return a, None, e, b

if __name__ == '__main__':
    for dirn in sys.argv[1:]:
        a, top, err, b = walk(dirn)
        name = os.path.basename(dirn.rstrip('/'))
        if err is None:
            print('%-32s OK   base=%d objs=%d end=%x/%x' % (name, b, len(a.all), a.p, len(a.d)))
        else:
            print('%-32s FAIL base=%d objs=%d at %x/%x: %s' % (name, b, len(a.all), a.p, len(a.d), err))
            print('   stack:', a.stack[-4:])
            if len(sys.argv) == 2:
                print(hexdump(a.d, max(0, a.p - 64), 256))
