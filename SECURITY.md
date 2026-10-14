# Security policy

## Project status

Crimson WM is **pre-alpha** software. There are no releases and no
supported production versions yet: only the latest state of the `main`
branch is maintained, and it is not intended for daily use.

## Scope

Crimson WM is designed to run only on isolated test X servers. Its startup
checks (rejecting display `:0`, requiring a sandbox token and marker, and
refusing to start if another window manager is present) are **safeguards
against accidents, not a security boundary**. All clients of an X11 server
can affect each other; this is a property of X11, not a vulnerability in
Crimson WM.

Reports are especially useful if they show that Crimson WM:

- can start or act on a display it should reject;
- can be made to crash, hang or consume unbounded resources by a client's
  window properties or requests;
- launches programs in an unsafe way (for example through a shell, or with
  environment that should have been removed);
- exposes credentials or personal data through its logs, the repository or
  the published documentation.

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

The intended private channel is GitHub's *Private Vulnerability Reporting*
for this repository ("Report a vulnerability" in the *Security* tab). It
may not be enabled yet; if it is not available, open a public issue that
only asks for a private contact, without including any details of the
problem.

Please include the commit you tested, the steps to reproduce it in an
isolated Xvfb server, and the impact you observed. This is a personal,
volunteer project: there is no guaranteed response time, but reports will
be taken seriously.
