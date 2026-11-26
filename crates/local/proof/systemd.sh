#!/usr/bin/env bash
# Proves `fun local setup` on real systemd: boots systemd 249 (Ubuntu 22.04) or
# 259 (Fedora 44) in a container, installs the unit `fun local setup --print` renders, and
# shows which lookups reach a resolver listening on the routing link's address — after enabling
# the unit, after restarting systemd-resolved, and after a reboot.
#
#   docker build -t fundamental-local-systemd:249 -f Dockerfile.249 .
#   docker build -t fundamental-local-systemd:259 -f Dockerfile.259 .
#   fun local setup --print | awk '/^# \/etc\/systemd/{f=1;next} /^# \/etc\/NetworkManager/{f=0} f' > unit
#   ./systemd.sh 249 unit; ./systemd.sh 259 unit
#
# Needs Docker and a privileged container (systemd as PID 1), never sudo on the host.
set -uo pipefail
v="$1"; unit="$2"; c="fundamental-local-proof-$v"
x() { docker exec "$c" "$@"; }
docker rm -f "$c" >/dev/null 2>&1
docker run -d --name "$c" --privileged --cgroupns=private --tmpfs /run --tmpfs /run/lock "fundamental-local-systemd:$v" >/dev/null
for _ in $(seq 60); do s=$(x systemctl is-system-running 2>/dev/null); case "$s" in running|degraded) break;; esac; sleep 1; done
echo "== systemd $(x systemctl --version | head -1 | cut -d' ' -f2) booted: $s"
x systemctl enable --now systemd-resolved >/dev/null 2>&1
x systemctl is-active systemd-resolved
resolver() { x sh -c '[ -f /tmp/d.pid ] && kill $(cat /tmp/d.pid) 2>/dev/null; sleep 0.3; : > /tmp/q.log; dnsmasq --user=root --pid-file=/tmp/d.pid --listen-address=192.0.2.53 --bind-interfaces --no-resolv --no-hosts --address=/test/127.0.0.9 --log-queries --log-facility=/tmp/q.log'; }
probe() {
  echo "-- $1"
  x resolvectl query --cache=no probe.acme.test 2>&1 | head -2
  x resolvectl query --cache=no www.fix-login.acme-lane.test 2>&1 | head -1
  x getent ahostsv4 www.fix-login.acme-lane.test | head -1
  x resolvectl query --cache=no example.net 2>&1 | head -1
  x getent hosts example.org >/dev/null; echo "getent example.org exit $?"
  echo "resolver saw: $(x sh -c "grep -o 'query\[A\] [^ ]*' /tmp/q.log | sort -u | tr '\n' ' '")"
  if ! x grep -q 'probe.acme.test' /tmp/q.log; then echo "FAIL: the .test lookup never reached the resolver";
  elif x grep -qE 'example\.(net|org)' /tmp/q.log; then echo "LEAK: a non-.test lookup reached the resolver";
  else echo "PASS: .test reached the resolver, and no non-.test lookup did"; fi
}
echo "-- control, before the unit: the link does not exist"; x ip -br link show fundamental-dns 2>&1 | head -1
docker exec -i "$c" tee /etc/systemd/system/fundamental-local-dns.service < "$unit" >/dev/null
x systemctl daemon-reload
x systemctl enable --now fundamental-local-dns.service 2>&1 | tail -1
x systemctl is-active fundamental-local-dns.service
x resolvectl status fundamental-dns 2>&1 | grep -E "Link|Current Scopes|DefaultRoute|DNS Servers|DNS Domain" 
resolver; probe "after enable"
pid_before=$(x cat /tmp/d.pid); x sh -c ': > /tmp/q.log'
x systemctl restart systemd-resolved; sleep 2
echo "-- after restarting systemd-resolved: unit $(x systemctl is-active fundamental-local-dns.service), domains: $(x resolvectl domain fundamental-dns), resolver pid $pid_before still $(x sh -c 'kill -0 $(cat /tmp/d.pid) && echo running')"
probe "after resolved restart, the resolver never restarted"
docker restart "$c" >/dev/null
for _ in $(seq 60); do s=$(x systemctl is-system-running 2>/dev/null); case "$s" in running|degraded) break;; esac; sleep 1; done
echo "-- after a reboot: unit $(x systemctl is-active fundamental-local-dns.service), domains: $(x resolvectl domain fundamental-dns), dns: $(x resolvectl dns fundamental-dns)"
resolver; probe "after reboot"
docker rm -f "$c" >/dev/null
