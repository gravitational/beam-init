import os
import time

# run 'cmd' in a forked subprocess
def forked(cmd):
    pid = os.fork()
    if pid == 0:
        cmd()
        os._exit(0)

# define a decorator that makes a function chatty
def verbose(delay):
    def decorator(func):
        def decorated():
            print(os.getpid(), "started", func.__name__)
            func()
            time.sleep(delay)
            print(os.getpid(), "ended", func.__name__)

        return decorated
    return decorator

@verbose(5)
def child():
    pass

@verbose(10)
def parent():
    forked(child)

# this creates a zombie child
parent()
