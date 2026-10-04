---
layout: tool
title: Interactive Cache Explorer
permalink: /cache-explorer/
lede: You are the CPU. Issue loads and stores to one L1 cache backed by main memory, and watch what happens on every access.
description: A teaching tool for one L1 cache. Issue loads and stores by hand and watch each access resolve, step by step.
---
<div id="cache-explorer" data-modules="{{ '/assets/cache-explorer/modules/index.json' | relative_url }}"></div>
<script type="module" src="{{ '/assets/cache-explorer/cache-explorer.js' | relative_url }}?v={{ site.time | date: '%s' }}"></script>
