# Roadmap

## Formulas

Hard
 - CONVERT(input, output, new format)
   - 3d model formats
   - media formats (gif,mp4,webm,webp,png,jpeg,bmp,tiff,psd,etc. <->)

Medium
 - GENERATE3D(input images)
 - FILTER (webgl image/video filters)

Easy
 - SPLIT(string, delimiter, output range)
 - SEPARATECSV(input, output range)
 - URL("https://...") -> extract image/model/text at url
 - GENERATESFX
 - GENERATETTS
 - GENERATEMUSIC
 - TRANSCRIBE
 - COMBINE -> combine audio and video
 - OVERLAY -> overlay images, video, etc
 - VECTORIZEIMAGE -> raster to vector
 - READURL -> read a page in a headless browser
 - VOXEL -> convert model to voxels (cubes)


## Features

- HuggingFace inference (as alternative to fal)
- Image file insertion (insert menu > image)
- Image file copy pasting from anywhere
- 3D model viewing in cells
- Gaussian splat formulas + viewing + manipulation
- 3d pipeline formulas (ex: voxelify, remesh, etc.)
- Undo/redo (menu + keyboard)
- Scripting / User defined formulas
 - js-like scripting with formula calls, http requests, etc.
- sound playback
- update current formula when changing models in docs
- Insert rows / columns between existing
- Text with minimal formating (ex: headings)

## Papercuts
 - triple click formula in docs selects page content - should select just <pre/
 - CMD+A in the sheet should select all cells rather than the current behaviour to select
  all html content
 - editing a formula causes run button to increment (should only compute once)

## Code cleanup

 -
