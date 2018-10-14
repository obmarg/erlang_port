# Changelog

All notable changes to this project will be documented in this file.

The format is roughly based on [Keep a
Changelog](http://keepachangelog.com/en/1.0.0/).

This project intends to inhere to [Semantic
Versioning](http://semver.org/spec/v2.0.0.html), but has not yet reached 1.0 so
all APIs might be changed.

## Unreleased - yyyy-mm-dd

## v0.2.0 - 2018-10-14

### Breaking Changes

- Completely refactored the interface to use structs and traits rather than top
  level functions. This should allow for users to implement their own protocols
  over traits in addition to the hard-coded 4 byte packet reading/writing that
  we had before.

### New Features

- We now support writing ports that are opened in nouse_stdio mode.

## v0.1.0 - 2018-10-13

The initial release.

### New Features

- Initial implementation with ability to send and receieve commands.
