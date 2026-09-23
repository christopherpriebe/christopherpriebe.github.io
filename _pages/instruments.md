---
layout: instruments
title: Instruments
permalink: /instruments/
nav: true
nav_order: 4
lede: Things you can take apart. Each one is a working model of something undergraduates are usually asked to picture in their heads, with every parameter exposed.

# Work not yet built, listed under the working instruments. `status` is the
# small note at the end of each row.
planned:
  - group: Computer architecture
    items:
      - title: Multi-core L1 data cache
        status: Planned · first
        description: Size, associativity and block size against core count, running snooping MSI or MOESI or a directory protocol. Victim cache later.
      - title: Single-cycle RV32I datapath
        status: Planned
        description: One instruction at a time, every control signal and every wire visible as it resolves.
      - title: Pipelined RV32I
        status: Planned
        description: The same datapath in five stages, with hazards, stalls and forwarding you can turn off to watch it break.
planned_note: Compilers and IR, and data structures and algorithms, are next up and have nothing in them yet.
---
