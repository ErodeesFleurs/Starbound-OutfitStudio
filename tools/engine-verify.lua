-- Private OpenStarbound onLoad fixture. No commercial asset bytes are bundled.
-- Run with the same external packed assets used by the Rust verify-assets CLI.
-- Asset loader catches script exceptions: acceptance requires PASS and no FAIL,
-- not merely the engine's exit status. This is NOT a vanilla-binary test.
local manifest = assets.json('/outfit-verification.json')
if manifest.format ~= 1 or manifest.frameSize ~= 43 then
  error('OUTFIT_VERIFY_FAIL unsupported manifest')
end

local channels = {'red', 'green', 'blue', 'alpha'}
local frameCount, variantCount, pixelCount, channelCount = 0, 0, 0, 0
local transparentCount = 0

local function fail(context, x, y, channel, expected, actual)
  local message = string.format(
    'OUTFIT_VERIFY_FAIL %s coordinate=(%d,%d) channel=%s expected=%s actual=%s',
    context, x, y, channel, tostring(expected), tostring(actual))
  sb.logError('%s', message)
  error(message)
end

local function pixel(image, x, y)
  local color = image:get(x, y)
  -- LuaConverter<Color> omits alpha for opaque colors, but never for alpha zero.
  if color[4] == nil then color[4] = 255 end
  return color
end

local function assertSize(image, context)
  local size = image:size()
  if size[1] ~= manifest.frameSize then fail(context, -1, -1, 'width', manifest.frameSize, size[1]) end
  if size[2] ~= manifest.frameSize then fail(context, -1, -1, 'height', manifest.frameSize, size[2]) end
end

for _, fixture in ipairs(manifest.cases) do
  for _, layer in ipairs(fixture.layers) do
    for _, pose in ipairs(layer.poses) do
      local sourcePath = layer.path .. ':' .. pose.name
      local context = fixture.name .. ' source=' .. sourcePath
      local source = assets.image(sourcePath)
      local canvas = assets.image(sourcePath .. fixture.originalPrefix)
      local baseline = assets.image(sourcePath .. fixture.baseline)
      assertSize(source, context .. ' source')
      assertSize(canvas, context .. ' original-template')
      assertSize(baseline, context .. ' original/off')
      -- Validate both real frame-name/fingerprint mapping and expected RGBA,
      -- independently of the encoder and of profile-to-profile equality.
      for y = 0, manifest.frameSize - 1 do
        for x = 0, manifest.frameSize - 1 do
          local address = {x, pose.id, y, 0}
          local actualCanvas = pixel(canvas, x, y)
          local input = pixel(source, x, y)
          local expected = input[4] == 0 and address or input
          local actualBaseline = pixel(baseline, x, y)
          for c = 1, 4 do
            if actualCanvas[c] ~= address[c] then
              fail(context .. ' original-template', x, y, channels[c], address[c], actualCanvas[c])
            end
            if actualBaseline[c] ~= expected[c] then
              fail(context .. ' original/off desired', x, y, channels[c], expected[c], actualBaseline[c])
            end
          end
          if input[4] == 0 then transparentCount = transparentCount + 1 end
        end
      end
      frameCount = frameCount + 1
      for _, variant in ipairs(fixture.variants) do
        local output = assets.image(sourcePath .. variant.directives)
        local variantContext = context .. ' variant=' .. variant.name
        assertSize(output, variantContext)
        for y = 0, manifest.frameSize - 1 do
          for x = 0, manifest.frameSize - 1 do
            local expected = pixel(baseline, x, y)
            local actual = pixel(output, x, y)
            for c = 1, 4 do
              if actual[c] ~= expected[c] then
                fail(variantContext, x, y, channels[c], expected[c], actual[c])
              end
              channelCount = channelCount + 1
            end
            pixelCount = pixelCount + 1
          end
        end
        variantCount = variantCount + 1
      end
    end
  end
end
if #manifest.cases > 0 then
  if frameCount == 0 or variantCount == 0 or pixelCount == 0 then
    error('OUTFIT_VERIFY_FAIL empty fixture')
  end
  sb.logInfo('OUTFIT_VERIFY_PASS fixtures=%s frames=%s variants=%s pixels=%s channels=%s transparent=%s',
    #manifest.cases, frameCount, variantCount, pixelCount, channelCount, transparentCount)
end

local hatVariants, hatPixels = 0, 0
local hats = manifest.hats or {}
for _, fixture in ipairs(hats) do
  local expected = assets.image(fixture.expectedPath)
  local size = expected:size()
  if size[1] ~= fixture.width or size[2] ~= fixture.height then
    error('OUTFIT_VERIFY_FAIL hat expected PNG dimensions')
  end
  for _, source in ipairs(fixture.sourcePaths) do
    for _, variant in ipairs(fixture.variants) do
      local context = fixture.name .. '/' .. source .. '/' .. variant.name
      local image = assets.image(source .. variant.directives)
      local outputSize = image:size()
      if outputSize[1] ~= size[1] or outputSize[2] ~= size[2] then
        error('OUTFIT_VERIFY_FAIL hat output dimensions ' .. context)
      end
      for y = 0, size[2] - 1 do
        for x = 0, size[1] - 1 do
          local wanted, actual = pixel(expected, x, y), pixel(image, x, y)
          for c = 1, 4 do
            if wanted[c] ~= actual[c] then
              fail(context, x, y, channels[c], wanted[c], actual[c])
            end
          end
          hatPixels = hatPixels + 1
        end
      end
      hatVariants = hatVariants + 1
    end
  end
end
if #hats > 0 then
  if hatVariants == 0 or hatPixels == 0 then error('OUTFIT_VERIFY_FAIL empty hat fixture') end
  sb.logInfo('OUTFIT_HAT_VERIFY_PASS fixtures=%s variants=%s pixels=%s channels=%s',
    #hats, hatVariants, hatPixels, hatPixels * 4)
end
local characterVariants, characterPixels = 0, 0
local characters = manifest.characters or {}
for _, fixture in ipairs(characters) do
  local expected = assets.image(fixture.expectedPath)
  assertSize(expected, fixture.name .. ' expected')
  for _, source in ipairs(fixture.sourcePaths) do
    local variants = assert(manifest.characterVariants[fixture.variantGroup], 'OUTFIT_VERIFY_FAIL missing character variant group')
    for _, variant in ipairs(variants) do
      local context = fixture.name .. '/' .. source .. '/' .. variant.name
      local image = assets.image(source .. variant.directives)
      assertSize(image, context)
      for y = 0, 42 do
        for x = 0, 42 do
          local wanted, actual = pixel(expected, x, y), pixel(image, x, y)
          for c = 1, 4 do
            if wanted[c] ~= actual[c] then
              fail(context, x, y, channels[c], wanted[c], actual[c])
            end
          end
          characterPixels = characterPixels + 1
        end
      end
      characterVariants = characterVariants + 1
    end
  end
end
if #characters > 0 then
  if characterVariants == 0 or characterPixels == 0 then error('OUTFIT_VERIFY_FAIL empty character fixture') end
  sb.logInfo('OUTFIT_CHARACTER_VERIFY_PASS fixtures=%s variants=%s pixels=%s channels=%s',
    #characters, characterVariants, characterPixels, characterPixels * 4)
end
if #manifest.cases == 0 and #hats == 0 and #characters == 0 then error('OUTFIT_VERIFY_FAIL empty manifest') end
