# Bevy OpenXR

A crate for adding openxr support to Bevy ( planned to be upstreamed ). 

To see it in action run the example in `examples` with `cargo run --example xr`

## This crate will not recive any feature or performance updates
the current implementation will not be updated!
we will release a version for bevy 0.14 once that releases but no further version will be supported!
there is a new rewrite that will be upstreamed into bevy in the future.
you can use that from this git repo if you want, but be warned it has a completly different public api.

## Discord

Come hang out if you have questions or issues 
https://discord.gg/sqMw7UJhNc

![](https://media.giphy.com/media/v1.Y2lkPTc5MGI3NjExY2FlOXJrOG1pbzFkYTVjZHIybndqamF1a2YwZHU3dXgyZGcwdmFzMiZlcD12MV9pbnRlcm5hbF9naWZfYnlfaWQmY3Q9Zw/CHbQyXOT5yZZ1VQRh7/giphy-downsized-large.gif)
![](https://media.giphy.com/media/v1.Y2lkPTc5MGI3NjExbHVmZXc2b3VhcGE2eHE2c2Y3NDR6cXNibHdjNjk5MmtyOHlkMXkwZyZlcD12MV9pbnRlcm5hbF9naWZfYnlfaWQmY3Q9Zw/Hsvp5el2o7tzgOf9GQ/giphy-downsized-large.gif)

## Troubleshooting

- Make sure, if you're on Linux, that you have the `openxr` package installed on your system.
- I'm getting poor performance.
    - Like other bevy projects, make sure you're building in release (example: `cargo run --example xr --release`)

## License

Unless otherwise specified, all code in this repository is dual-licensed under
either:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option. This means you can select the license you prefer!

### Your contributions

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
