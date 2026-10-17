# End-to-end test on Linux: needs `pip install pyte`. Run: cargo build && python3 scripts/e2e_linux.py
import pyte, os, pty, time, select, base64, subprocess, fcntl, termios, struct
CFG="/tmp/tmuxlike-e2e.toml"
open(CFG,"w").write('shell = "bash --norc --noprofile"\n[bind]\ny = "display hello-bind"\nv = "split-window -h"\n')
env=dict(os.environ, TMUXLIKE_SOCKET="tltest2.sock", TMUXLIKE_CONFIG=CFG, PS1="$ ", TERM="xterm-256color", TMUXLIKE_LANG="en", LANG="C.UTF-8", LC_ALL="C.UTF-8")
env.pop("TMUXLIKE",None)
B=os.environ.get("TMUXLIKE_BIN", os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "debug", "tmuxlike"))
def cli(*a): r=subprocess.run([B,*a],env=env,capture_output=True,text=True); return r.returncode, r.stdout+r.stderr
def run(args):
    pid, fd = pty.fork()
    if pid==0: os.execve(B,[B]+args,env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH",30,100,0,0))
    return fd
def rd(fd,t=0.8):
    out=b""; end=time.time()+t
    while time.time()<end:
        r,_,_=select.select([fd],[],[],0.05)
        if r:
            try: out+=os.read(fd,65536)
            except OSError: break
    ST.feed(out)
    return out
P=b"\x02"
SC=pyte.Screen(100,30); ST=pyte.ByteStream(SC)
def scr(): return '\n'.join(SC.display)
def chk(name, cond): print(("PASS " if cond else "FAIL ")+name)
fd=run(["new","-s","t1"]); rd(fd,1.5)
os.write(fd,P+b","); o=rd(fd); chk("rename-window prompt", b"(rename-window)" in o)
os.write(fd,b"\x15editor\r"); o=rd(fd); chk("rename-window", b"0:editor" in o)
os.write(fd,P+b"$"); rd(fd); os.write(fd,b"\x15work\r"); rd(fd)
chk("rename-session", "work:" in cli("ls")[1])
os.write(fd,P+b":"); o=rd(fd); chk("command prompt", b":" in o)
os.write(fd,b"split-window -h\r"); o=rd(fd); chk("prompt split-window", "│".encode() in o)
os.write(fd,P+b":bogus-cmd\r"); o=rd(fd); chk("prompt error msg", b"unknown command" in o)
os.write(fd,P+b"y"); o=rd(fd); chk("custom bind from config", "hello-bind" in scr())
os.write(fd,b"clear; echo COPYME\r"); rd(fd)
os.write(fd,P+b"["); rd(fd,0.4)
os.write(fd,b"k0Vy"); o=rd(fd)
chk("copy-mode V y -> OSC52", b"\x1b]52;c;"+base64.b64encode(b"COPYME\n") in o)
os.write(fd,P+b"]"); o=rd(fd); chk("paste-buffer", o.count(b"COPYME")>=1)
os.write(fd,b"\x03clear; echo DRAGTEXT\r"); rd(fd)
# active pane is the right pane (x from 51); output line row 2 (1-based y=2)
os.write(fd,b"\x1b[<0;52;1M\x1b[<32;55;1M\x1b[<32;59;1M\x1b[<0;59;1m"); o=rd(fd)
chk("mouse drag copy", b"\x1b]52;c;"+base64.b64encode(b"DRAGTEXT") in o)
open(CFG,"a").write('z2 = "x"\n'.replace('z2 = "x"\n',''))
open(CFG,"w").write('shell = "bash --norc --noprofile"\n[bind]\ny = "display reloaded-ok"\n')
os.write(fd,P+b"r"); o=rd(fd); chk("source-file message", b"sourced" in o)
os.write(fd,P+b"y"); o=rd(fd); chk("reloaded binding", b"reloaded-ok" in o)
os.write(fd,P+b"c"); rd(fd); os.write(fd,b"sleep 6\r"); o=rd(fd,4); chk("automatic rename", "1:sleep" in scr())
os.write(fd,P+b"s"); o=rd(fd); chk("chooser", b"Sessions" in o and b"work: 2 windows" in o)
os.write(fd,b"\x1b[A\r"); rd(fd)
os.write(fd,P+b"?"); os.write(fd,b"          "); o=rd(fd); chk("list-keys popup scroll", "new-window" in scr())
os.write(fd,b"q"); rd(fd)
os.write(fd,P+b":bind-key m display bound-at-runtime\r"); rd(fd)
os.write(fd,P+b"m"); o=rd(fd); chk("runtime bind-key", b"bound-at-runtime" in o)
os.write(fd,P+b":unbind m\r"); rd(fd); os.write(fd,P+b"m"); o=rd(fd); chk("unbind-key", b"bound-at-runtime" not in o)
os.write(fd,P+b"a"); o=rd(fd); chk("sidebar", b"work" in o and (b"idle" in o or b"working" in o))
chk("cli set", cli("set","mouse","off")[0]==0)
chk("cli bad cmd", cli("frob")[0]!=0)
os.write(fd,P+b"d"); o=rd(fd); chk("detach", b"detached" in o)
cli("kill-server")
