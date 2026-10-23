-- Loaded explicitly with `eval require('/vanilla-verify.lua')` over private RCON.
-- Uses only vanilla 1.4.4 root APIs. Checks every mapped pose's desired visible
-- RGBA and optimized color footprints, plus transparent-address samples. No networking.
local manifest = root.assetJson('/outfit-verification.json')
local expand = '?border=1;0000;0000?scalenearest=8?crop;4;4;348;348'

local function spaces(path)
  local size = root.imageSize(path)
  if size[1] ~= 43 or size[2] ~= 43 then error('OUTFIT_VANILLA_FAIL unexpected frame size') end
  return root.imageSpaces(path .. expand, {0, 0}, 0.5, false)
end

local function sameSpaces(expected, actual, context)
  if #expected ~= #actual then error('OUTFIT_VANILLA_FAIL occupancy count ' .. context) end
  local wanted = {}
  for _, p in ipairs(expected) do wanted[p[1] .. ',' .. p[2]] = true end
  for _, p in ipairs(actual) do
    local key = p[1] .. ',' .. p[2]
    if not wanted[key] then error('OUTFIT_VANILLA_FAIL pixel footprint ' .. context .. ' at ' .. key) end
    wanted[key] = nil
  end
end

function outfitVanillaVerify(caseIndex, firstPose, lastPose, firstVariant, lastVariant)
  local fixture = assert(manifest.cases[caseIndex], 'OUTFIT_VANILLA_FAIL unknown case')
  assert(firstPose >= 1 and lastPose >= firstPose, 'OUTFIT_VANILLA_FAIL invalid pose batch')
  local palette = {}
  local tail = fixture.baseline:sub(#fixture.originalPrefix + 1)
  for _, color in tail:gmatch('([%da-fA-F]+)=([%da-fA-F]+)') do
    local alpha = (#color == 3 or #color == 6) and 255 or tonumber(color:sub(-(#color == 4 and 1 or 2)), 16)
    if alpha > 0 then palette[color] = true end
  end
  local colors = {}
  for color in pairs(palette) do colors[#colors + 1] = color end
  table.sort(colors)
  local byName = {}
  for _, variant in ipairs(fixture.variants) do byName[variant.name] = variant end
  local variants = {}
  for _, name in ipairs({
      'conservative/auto', 'compact/auto', 'conservative/compressed',
      'conservative/passes', 'compact/compressed',
      'auto/text', 'auto/compressed', 'auto/passes'}) do
    variants[#variants + 1] = assert(byName[name], 'OUTFIT_VANILLA_FAIL missing native variant ' .. name)
  end
  assert(type(firstVariant) == 'number' and type(lastVariant) == 'number' and
    firstVariant >= 1 and lastVariant >= firstVariant and lastVariant <= #variants,
    'OUTFIT_VANILLA_FAIL invalid variant batch')
  local frames, checks, transparent = 0, 0, 0
  local poseIndex = 0
  for _, layer in ipairs(fixture.layers) do
    for _, pose in ipairs(layer.poses) do
      poseIndex = poseIndex + 1
      if poseIndex >= firstPose and poseIndex <= lastPose then
        local path = layer.path .. ':' .. pose.name
        local baseline = path .. fixture.baseline
        local occupied = spaces(baseline)
        sameSpaces(spaces(path), occupied, fixture.name .. '/' .. pose.name .. '/desired-occupancy')
        local oldMasks = {}
        for _, color in ipairs(colors) do
          oldMasks[color] = spaces(baseline .. '?replace;' .. color .. '=0000')
          sameSpaces(spaces(path .. '?replace;' .. color .. '=0000'), oldMasks[color],
            fixture.name .. '/' .. pose.name .. '/desired-RGBA/' .. color)
        end
        local exposed = {}
        for _, position in ipairs({{0, 0}, {21, 21}, {42, 42}}) do
          local suffix = string.format('?replace;%02x%02x%02x00=fff', position[1], pose.id, position[2])
          local mask = spaces(baseline .. suffix)
          if #mask > #occupied then exposed[#exposed + 1] = {suffix = suffix, mask = mask} end
        end
        for variantIndex = firstVariant, lastVariant do
          local variant = variants[variantIndex]
          local output = path .. variant.directives
          local context = fixture.name .. '/' .. pose.name .. '/' .. variant.name
          sameSpaces(occupied, spaces(output), context)
          for _, color in ipairs(colors) do
            sameSpaces(oldMasks[color], spaces(output .. '?replace;' .. color .. '=0000'), context .. '/' .. color)
            checks = checks + 1
          end
          for _, probe in ipairs(exposed) do
            sameSpaces(probe.mask, spaces(output .. probe.suffix), context .. '/transparent-RGB')
            transparent = transparent + 1
          end
        end
        frames = frames + 1
      end
    end
  end
  local result = {fixture = fixture.name, frames = frames, colorChecks = checks,
    transparentProbes = transparent, firstVariant = firstVariant, variants = lastVariant - firstVariant + 1}
  sb.logInfo('OUTFIT_VANILLA_BATCH_PASS %s', sb.printJson(result))
  return sb.printJson(result)
end

-- Deliberately unexpanded native query for first-access / metadata-cache timing.
-- Timing is performed around RCON by the runner, not claimed as render timing.
function outfitImageQuery(caseIndex, variantName)
  local fixture = assert(manifest.cases[caseIndex])
  local directives = fixture.baseline
  if variantName ~= 'original/off' then
    directives = nil
    for _, variant in ipairs(fixture.variants) do
      if variant.name == variantName then directives = variant.directives end
    end
    assert(directives, 'OUTFIT_VANILLA_FAIL unknown benchmark variant')
  end
  local layer = fixture.layers[1]
  local path = layer.path .. ':' .. layer.poses[1].name .. directives
  return #root.imageSpaces(path, {0, 0}, 0.5, false)
end

-- Hats have variable dimensions and an independent desired RGBA PNG. In
-- particular, revealing each exact color as opaque checks alpha=1 as well.
local function hatSpaces(path, fixture)
  local size = root.imageSize(path)
  if size[1] ~= fixture.width or size[2] ~= fixture.height then
    error('OUTFIT_VANILLA_FAIL hat dimensions ' .. fixture.name)
  end
  local expansion = string.format(
    '?border=1;0000;0000?scalenearest=8?crop;4;4;%d;%d',
    fixture.width * 8 + 4, fixture.height * 8 + 4)
  return root.imageSpaces(path .. expansion, {0, 0}, 0.5, false)
end

local function hatMask(fixture, wanted)
  local pairs = {}
  local found = false
  for _, color in ipairs(fixture.colors) do
    local selected = color == wanted
    found = found or selected
    pairs[#pairs + 1] = ';' .. color .. '=' .. (selected and 'fff' or '0000')
  end
  if not found then pairs[#pairs + 1] = ';' .. wanted .. '=fff' end
  return '?replace' .. table.concat(pairs)
end

function outfitVanillaHatVerify(caseIndex, sourceIndex, variantIndex)
  local fixture = assert(manifest.hats[caseIndex], 'OUTFIT_VANILLA_FAIL unknown hat')
  local source = assert(fixture.sourcePaths[sourceIndex], 'OUTFIT_VANILLA_FAIL unknown hat source')
  local variant = assert(fixture.variants[variantIndex], 'OUTFIT_VANILLA_FAIL unknown hat variant')
  local output = source .. variant.directives
  local context = fixture.name .. '/' .. source .. '/' .. variant.name
  sameSpaces(hatSpaces(fixture.expectedPath, fixture), hatSpaces(output, fixture), context .. '/occupancy')
  for _, color in ipairs(fixture.colors) do
    local mask = hatMask(fixture, color)
    local expected = hatSpaces(fixture.expectedPath .. mask, fixture)
    assert(#expected > 0, 'OUTFIT_VANILLA_FAIL unused hat palette color')
    sameSpaces(expected, hatSpaces(output .. mask, fixture), context .. '/RGBA/' .. color)
  end
  for _, probe in ipairs(fixture.transparent) do
    local c = probe.rgba
    local color = string.format('%02x%02x%02x%02x', c[1], c[2], c[3], c[4])
    local mask = hatMask(fixture, color)
    local expected = hatSpaces(fixture.expectedPath .. mask, fixture)
    assert(#expected > 0, 'OUTFIT_VANILLA_FAIL missing transparent hat sample')
    sameSpaces(expected, hatSpaces(output .. mask, fixture), context .. '/transparent-RGB')
  end
  if sourceIndex == 1 and variantIndex == 1 then
    for _, item in ipairs(fixture.descriptors) do
      local config = root.itemConfig(item)
      assert(type(config) == 'table' and type(config.config) == 'table',
        'OUTFIT_VANILLA_FAIL unrecognized hat item')
      assert((config.config.hideBody == true) == (item.name == 'frogghead'),
        'OUTFIT_VANILLA_FAIL hat hide-body base configuration')
    end
  end
  local result = {
    fixture = fixture.name, sourceIndex = sourceIndex, variantIndex = variantIndex,
    width = fixture.width, height = fixture.height,
    colorChecks = #fixture.colors, transparentProbes = #fixture.transparent
  }
  sb.logInfo('OUTFIT_VANILLA_HAT_BATCH_PASS %s', sb.printJson(result))
  return sb.printJson(result)
end

-- Characters use independent RGBA expectation PNGs for every real named frame,
-- including aliases. Reuse the exact-color isolation masks, not color removal
-- occupancy alone (which cannot prove alpha=1 or already opaque RGB).
function outfitVanillaCharacterVerify(caseIndex, sourceIndex, variantIndex)
  local fixture = assert(manifest.characters[caseIndex], 'OUTFIT_VANILLA_FAIL unknown character')
  local source = assert(fixture.sourcePaths[sourceIndex], 'OUTFIT_VANILLA_FAIL unknown character source')
  local group = assert(manifest.characterVariants[fixture.variantGroup], 'OUTFIT_VANILLA_FAIL missing character variant group')
  local variant = assert(group[variantIndex], 'OUTFIT_VANILLA_FAIL unknown character variant')
  local output = source .. variant.directives
  local context = fixture.name .. '/' .. source .. '/' .. variant.name
  sameSpaces(hatSpaces(fixture.expectedPath, fixture), hatSpaces(output, fixture), context .. '/occupancy')
  for _, color in ipairs(fixture.colors) do
    local mask = hatMask(fixture, color)
    local expected = hatSpaces(fixture.expectedPath .. mask, fixture)
    assert(#expected > 0, 'OUTFIT_VANILLA_FAIL unused character palette color')
    sameSpaces(expected, hatSpaces(output .. mask, fixture), context .. '/RGBA/' .. color)
  end
  for _, probe in ipairs(fixture.transparent) do
    local c = probe.rgba
    local color = string.format('%02x%02x%02x%02x', c[1], c[2], c[3], c[4])
    local mask = hatMask(fixture, color)
    local expected = hatSpaces(fixture.expectedPath .. mask, fixture)
    assert(#expected > 0, 'OUTFIT_VANILLA_FAIL missing transparent character sample')
    sameSpaces(expected, hatSpaces(output .. mask, fixture), context .. '/transparent-RGB')
  end
  local result = {
    fixture = fixture.name, sourceIndex = sourceIndex, variantIndex = variantIndex,
    width = fixture.width, height = fixture.height,
    colorChecks = #fixture.colors, transparentProbes = #fixture.transparent
  }
  sb.logInfo('OUTFIT_VANILLA_CHARACTER_BATCH_PASS %s', sb.printJson(result))
  return sb.printJson(result)
end
