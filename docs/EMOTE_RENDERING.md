# Native emote rendering conventions

The bundled SPEmotes JSONs omit `version`, so they use Emotecraft format v1. In v1/v2, `torso` names the **whole-body root**, not the chest bone. Format v3 adds a separate `torso` model part. Root translation is measured in Minecraft blocks; limb translations are measured in model pixels (16 pixels per block). The root pivot is 0.7 blocks above the feet.

`EmoteClip` decodes this distinction before sampling. It honors the documented `easeBeforeKeyframe` flag (default false), degree/radian units, begin/stop ticks, string or boolean loop flags, and return ticks. The preview intentionally plays one cycle of each bundled clip, with a short orientation/position fade to the neutral pose between clips. Quaternion interpolation is used for these fades; authored Euler keyframes keep their own timing and turn values.

The Qt renderer uses Y-up coordinates with the front face toward positive Z. Model-part rotations and root rotations need different coordinate conversions because root motion happens before Minecraft reflects its model coordinates. `modelTransform` and `bodyTransform` keep this distinction explicit and covered by tests.

Joint deformation uses the BendyLib closed joint-plane construction, not per-vertex weighted rotation. Both halves share the joint plane while their far ends preserve the rigid transform. The center is the actual cuboid center, including classic/slim arm offsets and the outer layer expansion. A body bend transforms the head and arms rigidly as complete upper-body children; it must not deform the individual vertices of a hanging arm a second time.

References (MIT notice in `licenses/animation-reference-MIT.txt`):

- [PlayerAnimator AnimationJson](https://github.com/KosmX/minecraftPlayerAnimator/blob/cb3227efc19ec46065597332ae265076d0f2b495/coreLib/src/main/java/dev/kosmx/playerAnim/core/data/gson/AnimationJson.java)
- [PlayerAnimator root transform](https://github.com/KosmX/minecraftPlayerAnimator/blob/cb3227efc19ec46065597332ae265076d0f2b495/minecraft/common/src/main/java/dev/kosmx/playerAnim/mixin/PlayerRendererMixin.java)
- [BendyLib joint construction](https://github.com/KosmX/bendy-lib/blob/100125490d6c4f0e707678375b2923aae1f3de9a/common/src/main/java/io/github/kosmx/bendylib/impl/IBendable.java)

Run `emote-test -platform offscreen`. `CK_UI_SCREENSHOTS` selects the output directory, and the optional `CK_TEST_SKIN` loads a real local PNG for manual review. Personal textures must not be committed or included in CI artifacts; CI uses a generated test texture by default. The tests cover all phases of all four shipped clips, classic/slim/64×32 models, continuous preview bounds, root movement, easing, loop boundaries, and joint continuity.
