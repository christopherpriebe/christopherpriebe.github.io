---
layout: base
title: GeneSys
description: >
    An open-source parameterizable neural processing unit (NPU) generator with a full-stack multi-target compilation stack.
    It allows users to produce complete acceleration systems for emerging deep learning models, such as convolutional neural networks (CNNs) and transformers, with minimal human intervention.
img: genesys_logo.jpg
importance: 1
---

GeneSys is a full-stack system for accelerating deep learning models like convolutional neural networks (CNNs) and transformers-based language models.
It comprises a parameterizable NPU generator, which has been both taped out and prototyped on AWS F1 FPGAs, a full-stack multi-target compilation stack, and an RTL verification framework with a regression suite that includes synthetic and state-of-the-art DNN benchmarks like ResNet50, BERT, and GPT2.
Additionally, GeneSys includes OpenCL-based Linux drivers, hardware synthesis scripts, a software simulator for profiling, and user-friendly Python APIs.

My primary contribution to GeneSys has been the continued development of the system's compiler, which was originally written by Sean Kinzer.

For more information, see the <a href="https://actlab-genesys.github.io/" target="_blank">GeneSys website</a>.
