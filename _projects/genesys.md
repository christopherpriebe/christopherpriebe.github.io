---
layout: single_column_page 
title: GeneSys
show_title: true
sub_title: Open-Source Parameterizable NPU Generator with Full-Stack Multi-Target Compilation Stack
description: >
    An open-source parameterizable neural processing unit (NPU) generator with a full-stack multi-target compilation stack.
    It allows users to produce complete acceleration systems for emerging deep learning models, such as convolutional neural networks (CNNs) and transformers, with minimal human intervention.
website: https://actlab-genesys.github.io/ 
github: https://github.com/actlab-genesys/GeneSys
importance: 1
---

<figure>
    <img src="/assets/images/genesys_flow.jpg" class="img-rounded img-responsive center-block" alt="GeneSys workflow." style="width: 60%; padding-bottom: 15px;">
    <figcaption class="text-center">
        <p>GeneSys workflow.</p>
    </figcaption>
</figure>

One of the major enabling factors in the significant advancement of deep learning (like convolutional and transformer-based neural networks) is the rapid growth of computing power in the 2010s.
With the end of Dennard scaling {% cite dennard-scaling:jssc:1974 --file genesys.bib %} and the advent of dark silicon {% cite dark-silicon:isca:2011 --file genesys.bib %}, research and development has shifted towards adopting hardware accelerators for deep learning {% cite diannao:asplos:2014 dnnweaver:micro:2016 eyeriss:isca:2016 simba:micro:2019 gemmini:dac:2021 --file genesys.bib %}.
Deep neural network (DNN) accelerators have found their way into production datacenters {% cite tpu:isca:2017 facebook-inference-accelerator:arxiv:2021 tpuv4i:isca:2021 --file genesys.bib %}, autonomous vehicles {% cite auto-driving-accelerator:asplos:2018 --file genesys.bib %}, internet of things (IoT) devices {% cite minerva:isca:2016 iot-accelerator:jssc:2018 --file genesys.bib %}, and biomedical devices {% cite ulp-srp:trets:2014 bioaip:isscc:2021 --file genesys.bib %}.
Besides the challenging task of designing hardware accelerators that abide by various power, performance, and area constraints, there is also ongoing research on how to seamlessly integrate hardware accelerators in the software stack {% cite optimus:asplos:2020 ava:asplos:2020 coyote:osdi:2020 --file genesys.bib %}.
Therefore, we must shift the focus from standalone hardware to holistic system design.

The Alternative Computing Technologies (ACT) Lab, led by my advisor Hadi Esmaeilzadeh, is one of the few in academia to have developed a fully fledged programmable accelerator generator, called GeneSys.
GeneSys is a full-stack system designed to accelerate deep learning models such as convolutional neural networks (CNNs) and transformers-based language models.
It comprises a parameterizable neural processing unit (NPU) generator capable of creating hardware accelerators with various configurations, which has been both taped out and prototyped on AWS F1 FPGAs.
GeneSys also features a multi-target compilation stack that supports algorithms beyond deep learning, OpenCL-based Linux drivers, user-friendly Python APIs, and an RTL verification framework with a regression suite including synthetic and state-of-the-art DNN benchmarks like ResNet50, BERT, and GPT2.
Additionally, it includes hardware synthesis scripts, a software simulator for profiling, and comprehensive software support.
These unique and functional artifacts represent years of research and multiple published papers by the ACT Lab {% cite tabla:hpca:2016 dnnweaver:micro:2016 bit-fusion:isca:2018 planaria:micro:2020 polymath:hpca:2021 yin-yang:ieeemicro:2022 --file genesys.bib %}.
GeneSys has already been used in multiple published papers {% cite dmx:hpca:2024 tandem:asplos:2024 dscs:asplos:2024 --file genesys.bib %} and course projects in CSE 240D at the University of California San Diego.

My primary contribution to GeneSys has been the continued development of the system's compiler, which was originally written by Sean Kinzer.
After taking on the mantle, I streamlined the setup and installation process by packaging the compiler with pip, implemented a new greedy memory allocation strategy based on {% cite greedy-memory-alloc-nn-inference:arxiv:2020 --file genesys.bib %} which significantly reduced the memory footprint in DRAM during execution, removed excess code bloat leftover from stitching multiple projects together during the initial development phase, and expanded the neural network layer support of the compiler to accommodate more diverse models.
I also had the opportunity to give oral presentations on the compiler through tutorials organized by ACT Lab.
The tutorials were presented at the following venues:
- *IEEE/ACM International Symposium on Microarchitecture* (MICRO) on October 29th, 2023 in Toronto, Canada
- *IEEE International Symposium on High-Performance Computer Architecture* (HPCA) on March 2nd, 2024 in Edinburgh, Scotland
- *ACM International Conference on Architectural Support for Programming Languages and Operating Systems* (ASPLOS) on April 28th, 2024 in San Diego, California
- *International Symposium on Computer Architecture* (ISCA) on June 29th, 2024 in Buenos Aires, Argentina.

For more information on the project as a whole, see the [GeneSys website](https://actlab-genesys.github.io/).

{% include blog_bib.liquid %}
