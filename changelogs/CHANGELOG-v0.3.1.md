The v0.3.1 release is a patch release which includes some bug fixes and makes
the ability for BPF enabled applications to receive their maps via bpfman's
custom CSI plugin default.

This enablement means that most applications can now access their maps **WITHOUT**
being run as root.

> [!WARNING]
The CSI feature still requires a privileged application on distributions which
enable SELinux by default (i.e Red Hat Openshift).  Therefore we've shipped a set of
deployment configs specifically for openshift in this release, see the additional
`go-<example>-counter-install-ocp-v0.3.1.yaml` artifacts included in the release
payload. Stay tuned to [#829](https://github.com/bpfman/bpfman/issues/829) for
updates.

The new yaml syntax that should be used by BPF enabled applications resembles
the following:

```yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: <APP_NAME>
  ...
spec:
  ...
  template:
    ...
    spec:
     ...
          volumeMounts:
            - name: bpf-maps
              mountPath: /bpf-maps
      volumes:
        - name: bpf-maps
          csi:
            driver: csi.bpfman.io
            volumeAttributes:
              csi.bpfman.io/program: <*Program_Name>
              csi.bpfman.io/maps: <BPF Map Names>
```

Additionally, this release removes all dependencies involved with deploying bpfman
with TLS, which means that cert-manager dependencies are completely removed from
the operator, therefore simplifying the deployment considerably.

Lastly, the bpfman user and user group was removed which will only effect users
that run bpfman via a systemd service and try to use `bpfctl` without root
privileges.  This helped reduce internal complexity and allows us to focus instead
on finetuning the permissions of the bpfman process itself, see the [linux
capabilities guide](https://bpfman.io/developer-guide/linux-capabilities/) for more information.

## What's Changed (excluding dependency bumps)
* release: automate release yamls by @astoycos in https://github.com/bpfman/bpfman/pull/775
* bpf: returns an error when adding a tc program to existence clsact qdisc by @navarrothiago in https://github.com/bpfman/bpfman/pull/761
* workspace-ified the netlink dependencies by @anfredette in https://github.com/bpfman/bpfman/pull/783
* Don't try and pin .data maps by @astoycos in https://github.com/bpfman/bpfman/pull/794
* .github: Add actions to dependabot by @dave-tucker in https://github.com/bpfman/bpfman/pull/803
* Fix Procceedon bug (Issue #791) by @anfredette in https://github.com/bpfman/bpfman/pull/792
* Add script to delete bpfman qdiscs on all interfaces by @anfredette in https://github.com/bpfman/bpfman/pull/780
* Fix BPF Licensing by @dave-tucker in https://github.com/bpfman/bpfman/pull/796
* Fix example bytecode image builds add test coverage by @astoycos in https://github.com/bpfman/bpfman/pull/810
* Relicense userspace to Apache 2.0 only by @dave-tucker in https://github.com/bpfman/bpfman/pull/795
* bpfman: Use tc dispatcher from container image by @dave-tucker in https://github.com/bpfman/bpfman/pull/817
* bpfman: Unify the "run as root" and "run as bpfman user" codepaths by @Billy99 in https://github.com/bpfman/bpfman/pull/777
* bpfman, bpfctl, operator: Remove support for TCP/TLS by @dave-tucker in https://github.com/bpfman/bpfman/pull/819
* bpfman-operator: Make the CSI deployment default for bpfman-operator by @Billy99 in https://github.com/bpfman/bpfman/pull/811
* ci: Add YAML formatter by @dave-tucker in https://github.com/bpfman/bpfman/pull/802
* Fix some panics + add testing and fix for map sharing  by @astoycos in https://github.com/bpfman/bpfman/pull/820
* bpfman: mount default bpffs on kind by @astoycos in https://github.com/bpfman/bpfman/pull/823
* bpfman: Remove unused file by @dave-tucker in https://github.com/bpfman/bpfman/pull/824
* Document valid kernel versions by @Billy99 in https://github.com/bpfman/bpfman/pull/827
* Update documentation on new YAML Linter by @Billy99 in https://github.com/bpfman/bpfman/pull/830

**Full Changelog**: https://github.com/bpfman/bpfman/compare/v0.3.0...v0.3.1
