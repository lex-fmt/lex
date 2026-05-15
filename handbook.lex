The High-End Photo Editor:
A Handbook of Architecture, Color Science, and Image Processing

    This handbook describes the design and implementation of modern high-end photographic editors. It covers the journey from raw sensor data to final export, the color science that makes images look right, and the architectural patterns that make 100-megapixel files tractable on consumer hardware.

    The material is organized in five parts. The primer introduces the design space and the core concepts (color spaces, perceptual uniformity, linear versus display-referred data). The pipeline overview gives a single-page map from Bayer sensor to exported pixel. The chapter-per-stage section then walks through each stage in depth. Two focused chapters cover excellence in raw processing and excellence in color science specifically. A competitive landscape chapter and a resources appendix close the volume.

    1. Glossary

        Terms are listed alphabetically. Where a term has a mathematical definition, the formula is given inline.

        AgX:
            A scene-to-display tone-mapping operator (a "view transform") that emulates the response of silver-halide film. AgX is built from three parts: a 3x3 _inset matrix_ that pulls pure RGB primaries toward neutral gray, a #\log_2# mapping that compresses scene-linear values into a log domain, and a 1D _S-curve_ applied to each channel. Its defining property is a graceful highlight _shoulder_ where contrast gradually decreases as values approach white, producing the "filmic" roll-off that digital pipelines otherwise lack. AgX originated in the open-source community (Blender, Darktable) as an alternative to ACES for non-cinematic stills work.

        Bayer Pattern:
            The mosaic color filter array (CFA) covering most digital sensors. Each photosite is covered by a single red, green, or blue filter in a repeating 2x2 pattern (typically RGGB), giving green twice the sample density of red and blue. The image must be _demosaiced_ before it can be displayed.

        Bilateral Filter:
            An edge-aware blur. Standard Gaussian blur averages a pixel with its neighbors weighted by spatial distance only. A bilateral filter additionally weights by intensity distance, so pixels across a strong edge contribute less. Result: smoothing happens within regions but not across edges. Used as the base layer in frequency-separation tonal operations.

        Black Level:
            The raw sensor value corresponding to "no light." Not zero â€” sensors always read a small positive bias to keep noise from clipping into negative numbers. The black level must be subtracted before any linear math is meaningful. Modern sensors report this per-shot by reading _optical black pixels_.

        CLAHE (Contrast Limited Adaptive Histogram Equalization):
            A localized contrast operation. The image is divided into a grid (typically 8x8), a cumulative distribution function (CDF) is computed for each tile, and a localized tonal curve is derived from that CDF. Tiles are blended with bilinear interpolation so grid boundaries are invisible. The _contrast limit_ clips the histogram before CDF computation, preventing flat regions (skies) from being equalized into harsh grain.

        Channel Mixer:
            A black-and-white conversion tool that lets the user weight R, G, and B contributions independently. Crude approximation of physical film's spectral sensitivity. Modern pipelines prefer spectral remapping nodes operating in linear RGB before perceptual transform.

        Chromatic Aberration (CA):
            Lens-induced misregistration of color channels. _Longitudinal CA_ shows as color fringing on out-of-focus elements; _transverse CA_ (TCA) shows as red/cyan or blue/yellow edges scaling outward from the optical center. Correctable with lens-profile polynomials applied per-channel.

        ColorChecker / IT8 Target:
            Standardized printed targets with patches of known spectral reflectance. Photographed under known illuminants (A, D65), they serve as ground truth for building camera input profiles.

        Color Constancy:
            A property of human vision: the perceived color of an object remains stable across changes in illumination. A white shirt looks white in tungsten or daylight. Cameras lack this property and must compensate via _white balance_.

        ColorMatrix1 / ColorMatrix2:
            DNG metadata tags storing 3x3 matrices that convert camera raw RGB to XYZ under two reference illuminants (typically A and D65). The decoder interpolates between them based on the shot's estimated white point.

        Curves:
            A 1D transfer function #P_{out} = f(P_{in})# implemented as a spline (Catmull-Rom or cubic Bezier) or a lookup table. Mathematically subsumes exposure, brightness, and contrast â€” those operations are special cases of curves with constrained shapes.

        DAG (Directed Acyclic Graph):
            The data-flow representation of a non-destructive editor. Each node is an operation; edges carry pixel data. Execution order is determined by graph topology rather than UI position. Examples: Nuke, DaVinci Resolve, Darktable's pixel pipe.

        Dark Channel Prior:
            An empirical observation underlying most _dehaze_ algorithms: in a clear, non-hazy outdoor patch of an image, at least one RGB channel is typically very dark (near zero). Haze raises this floor uniformly, and the difference between the observed and expected dark channel estimates haze thickness per pixel.

        Dark Frame:
            An exposure taken with the shutter closed, capturing only sensor read noise and thermal signal. Subtracted from a long exposure to remove fixed-pattern noise. In stills cameras this is automated via _Long Exposure Noise Reduction_ (LENR).

        Debayering / Demosaicing:
            Reconstructing a full RGB image from the Bayer mosaic. Naive bilinear interpolation produces _zipper artifacts_ and _false-color moirÃ©_ at edges. Modern algorithms (AHD, RCD, AMaZE, AI-based DeepPRIME) are direction-aware: they compute edge gradients in horizontal, vertical, and diagonal directions before deciding how to interpolate missing values.

        Delta E (#\Delta E#):
            A scalar metric for perceptual color difference between two color values. Defined in a perceptually uniform space (originally CIE Lab, now often CIEDE2000 or #\Delta E_{OK}# in OKLab). A #\Delta E# of 1.0 is approximately the threshold of human perception under controlled conditions.

        Dehaze:
            A spatial operation that inverts atmospheric scattering. Models haze as a per-pixel veil of additive light and a transmission map estimating veil thickness, then solves #J(x) = (I(x) - A) / t(x) + A# for the recovered image #J# given observed image #I#, atmospheric light #A#, and transmission map #t#.

        DNG OpCodes:
            Instructions embedded in DNG raw files describing per-lens corrections: distortion polynomials, vignetting gain maps, chromatic aberration coefficients. Generated by the camera based on its lens profile database. Mathematically precise for the specific lens/body combination.

        DOD (Domain of Definition) / ROI (Region of Interest):
            In a pull-based DAG, the DOD is the region where a node has valid output; the ROI is the region a downstream node requests. For spatial nodes (blur, clarity), the ROI must be expanded by the operation's radius to provide a _halo_ of source pixels for edge calculation.

        Dual Illuminant Profile:
            A camera profile defined under two illuminants (typically A and D65), with the decoder interpolating between them based on the white point of the current shot. Standard in DNG. Necessary because sensor spectral response is not a linear combination of human cone response (_Luther-Ives violation_), so a single matrix cannot be accurate under all lighting.

        Edge-Aware Filter:
            Any filter whose smoothing strength varies with local image gradient. Bilateral, guided, and domain-transform filters are the common implementations. Foundational to modern shadows/highlights tools, dehaze, and clarity.

        Exposure:
            Linear scalar multiplication #P_{out} = P_{in} \cdot 2^E# where #E# is exposure value in stops. The only operation that physically corresponds to changing captured light. Must be applied in scene-linear RGB before any non-linear math.

        False Color / MoirÃ©:
            Demosaicing artifacts. MoirÃ© is interference between the Bayer pattern's sampling frequency and high-frequency content in the scene (fine fabric, brick); false color is a related artifact where chromatic noise appears on luminance edges. Mitigated by direction-aware demosaic algorithms and, when present, the sensor's optical low-pass (anti-aliasing) filter.

        Feature Fusion / Conditioning:
            A neural-network input strategy where the model receives more than the standard 3-channel RGB tensor. Additional channels carry auxiliary signals (the user's current edited L channel, a high-frequency edge map, a depth estimate). The network learns to use the proxy channels for semantic identification and the conditioning channels for spatial precision.

        Filmic / Filmic RGB:
            Darktable's scene-referred tone-mapping module. Conceptually a predecessor to AgX: log-encode the scene, apply a shoulder/toe S-curve, decode to display. The defining commitment is keeping all editing math in scene-linear space until the very end of the pipeline.

        Flat Frame:
            In astrophotography, a uniformly-lit exposure used to characterize vignetting and dust. In stills the equivalent is a _synthetic flat_ derived from the lens profile (focal length, aperture, distance from optical center) rather than a captured frame.

        Frequency Separation:
            Decomposition of an image into a low-frequency _base layer_ (broad tonal variations) and a high-frequency _detail layer_ (edges and texture). Implemented via edge-aware filters: #Image = Base + Detail#. Manipulations on each band can be reapplied independently. Foundational to clarity, texture, micro-contrast, and modern shadows/highlights.

        Gamma / OETF:
            The non-linear encoding curve that maps scene-linear values to display code values. sRGB uses a piecewise function approximating #P^{1/2.2}#. The "opto-electronic transfer function" name reflects that this is fundamentally a property of how displays emit light, not an aesthetic choice.

        Gamut:
            The set of colors a system can represent. Camera sensors have wide gamuts; displays (sRGB, Display P3, Rec.2020) have progressively wider gamuts but all narrower than the visible spectrum. Out-of-gamut values must be _gamut-mapped_ (clipped, compressed, or perceptually remapped) before display.

        Golden Path:
            A default node sequence in a node-based editor that enforces mathematically sound ordering (white balance before exposure, dehaze before contrast, texture last). Users may reorder, but defaults prevent the common errors that produce muddy or artifact-laden results.

        Guided Filter:
            An edge-preserving filter that uses one image as a "guide" to smooth another. Cheaper than bilateral filtering for large radii and widely used in clarity, dehaze, and shadow/highlight implementations.

        Halo Artifact:
            A bright or dark fringe around edges, typically caused by aggressive local contrast operations (clarity, unsharp mask, naive HDR tone mapping). In silver-halide film, controlled halos are called _Mackie lines_ and are considered aesthetically desirable; in digital, uncontrolled halos read as "overcooked."

        Highlight Roll-off:
            The non-linear behavior of bright values as they approach clipping. Silver-halide film compresses gradually (long shoulder); digital sensors clip abruptly. Filmic tone mappers (AgX, Filmic RGB) synthesize a shoulder mathematically to mimic film.

        Histogram Equalization:
            A global contrast operation that remaps pixel values so the output histogram approximates a uniform distribution. Maximizes contrast but typically destroys image character. _CLAHE_ is the localized, contrast-limited variant that actually works in practice.

        ICC Profile:
            The International Color Consortium's standard for describing color spaces. An ICC profile contains the matrices and/or LUTs needed to convert between a device's color space and a connection space (PCS, typically XYZ or Lab). Capture One's color science relies heavily on per-camera ICC profiles.

        Illumination Map / Intrinsic Image Decomposition:
            A spatial estimate of the color temperature and intensity of light hitting each region of a scene, separated from the underlying surface reflectance. Enables local white balance for mixed-lighting scenes (e.g., daylight and tungsten in the same frame).

        Inset Matrix:
            In AgX, the 3x3 matrix that pulls pure RGB primaries toward neutral gray before the log encoding. Controls AgX's "path to white" behavior â€” how saturated colors desaturate as they approach clipping. The nine values of this matrix are the primary tunable parameters when calibrating AgX to specific aesthetic targets.

        JzAzBz:
            A perceptually uniform color space designed for HDR content. Like OKLab in intent â€” separating luminance from chromaticity â€” but with better behavior in the very-bright and very-dim extremes that HDR pipelines push into.

        Latent Style Vector:
            In AI-assisted editors, a learned embedding representing an image's aesthetic state. Edits move the image through a high-dimensional style space; refinement loops interpolate between vectors representing user-preferred outcomes.

        LERP (Linear Interpolation):
            #P_{new} = P_a + (P_b - P_a) \cdot t# for #t \in [0, 1]#. The basic primitive of parameter blending in refinement loops, layer compositing, and pyramid level transitions.

        Lensfun:
            An open-source (LGPL/GPL) database of lens correction polynomials. Community-maintained XML files describe distortion, vignetting, and TCA for thousands of lenses. Used as a fallback when embedded DNG OpCodes are absent (e.g., adapted vintage glass).

        LibRaw:
            An open-source library (derived from dcraw) that handles the low-level work of reading manufacturer-specific raw formats. Extracts sensor data, applies black-level subtraction, reads ColorMatrix tags, and exposes the result as linear RGB.

        Linear RGB / Scene-Referred:
            Pixel values proportional to the scene's photon counts. Doubling a linear value corresponds to doubling the light. All physically meaningful operations (exposure, white balance, dehaze) must be performed in linear RGB to be mathematically correct.

        LMS Color Space:
            A color space whose three axes correspond to the responses of the long, medium, and short cone cells in the human retina. An intermediate stop in many perceptual color transforms (linear RGB to LMS to OKLab).

        Long Exposure Noise Reduction (LENR):
            An in-camera process: after a long exposure, the shutter remains closed and the camera takes a second exposure of identical duration. The second frame captures thermal noise and fixed-pattern noise alone, which is subtracted from the first. Doubles capture time but produces cleaner shadows.

        LUT (Lookup Table):
            A precomputed table mapping input values to output values. 1D LUTs are used for tone curves; 3D LUTs (often 33x33x33 or 65x65x65) map RGB triplets through arbitrary transformations including memory-color twists and color grades. Evaluated via _tetrahedral interpolation_ for smooth results.

        Luther-Ives Condition:
            A theoretical requirement that a camera's spectral sensitivities be a linear combination of the human cone responses. If satisfied, a 3x3 matrix can perfectly map sensor RGB to perceived color. No real sensor satisfies it, so all camera profiles include non-linear corrections (3D LUTs or dual-illuminant interpolation) to handle hue shifts that a matrix alone cannot.

        Mackie Lines:
            The contrast-enhanced edges produced by certain film developers (notably Rodinal). Subjectively desirable: they create perceived sharpness without the harshness of digital sharpening.

        Memory Color:
            A color the human visual system has strong expectations for, independent of measurement: skin tones, sky blue, foliage green, neutral gray. Camera profiles deliberately deviate from colorimetric accuracy in these regions to match expectation rather than reality.

        Mipmap / Image Pyramid:
            A series of progressively downsampled versions of an image (1/2, 1/4, 1/8, ...). Stored together, the pyramid lets a renderer pick the resolution that matches the viewport, avoiding work on pixels that will never be displayed.

        Multi-Headed Inference:
            An architecture where a single input produces several plausible outputs (different exposure choices, different color grades) rendered as low-resolution previews. The user selects, and the system narrows toward the selected branch via successive refinement.

        Notorious Six:
            The six fully-saturated primaries and secondaries (R, G, B, C, M, Y) at the corners of the RGB cube. Notorious because most tone-mapping operators handle them poorly: they don't desaturate naturally as exposure increases, instead producing harsh hue shifts. AgX's inset matrix is designed primarily to fix this.

        OKLab:
            A perceptually uniform color space designed by BjÃ¶rn Ottosson (2020). Three axes: L (lightness), a (green-red), b (blue-yellow). Critically, equal numerical distances in OKLab correspond to equal perceived distances, which is not true of older spaces like CIE Lab. The current standard for tonal and chromatic edits in modern editors.

        OETF (Opto-Electronic Transfer Function):
            See _Gamma_. The encoding function applied at display time.

        Optical Black Pixels:
            Rows or columns of sensor pixels covered by an opaque shield. They receive no light and serve as a per-shot measurement of the current black level, accounting for temperature drift.

        Path to White:
            The trajectory taken by saturated colors as they approach clipping. Filmic tone mappers desaturate gradually so highlights become white rather than clipping into pure primary colors. Controlled by AgX's inset matrix.

        Perceptually Uniform Space:
            A color space where Euclidean distance approximates perceptual difference. OKLab, JzAzBz, and (less successfully) CIE Lab are perceptually uniform; sRGB and linear RGB are not. Required for any tonal operation that should "look smooth" rather than just be mathematically smooth.

        Pixel Pipe:
            Darktable's term for its node-based DAG. Distinguishes from monolithic editing pipelines: each module is a separate node with explicit inputs, outputs, and a configurable position in the chain.

        Pointwise Operation:
            An operation where each output pixel depends only on its own input value: exposure, curves, white balance. Contrasts with _spatial operations_ (blur, clarity, denoise) where output depends on neighborhoods.

        Pull Model / Demand-Driven Evaluation:
            A DAG execution strategy where the display drives computation: the viewport requests a region from the final node, which recursively requests source pixels from its parents. Only the regions and resolutions actually displayed are computed. Standard in high-end systems (Nuke, Resolve, Capture One).

        Push Model / Eager Evaluation:
            The opposite of pull: each node, on parameter change, immed
