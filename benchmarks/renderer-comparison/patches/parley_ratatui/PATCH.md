# `parley_ratatui` compatibility patch

This directory is `parley_ratatui` 0.3.4 at upstream commit
`2c40e50ac0ac704f23432726ded0b081876adbd3`, with only the following change:

- implement `Backend::scroll_region_up` and `Backend::scroll_region_down`,
  which are exposed when this workspace enables Ratatui 0.30.2's
  `scrolling-regions` feature.

The new implementations only move/reset retained buffer cells. None of the
Parley shaping, Vello scene construction, GPU submission, or benchmarked hot
paths were changed.

Upstream: <https://github.com/gold-silver-copper/parley_ratatui>
