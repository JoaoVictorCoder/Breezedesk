-- Turns the WMO weather code from open-meteo into a description and an icon.
-- Runs in LuaJIT; `update()` returns variables the widget can use: {desc} {icon}

local codes = {
  [0] = { "Clear sky", "☀" },
  [1] = { "Mainly clear", "☀" },
  [2] = { "Partly cloudy", "⛅" },
  [3] = { "Overcast", "☁" },
  [45] = { "Fog", "☁" },
  [48] = { "Rime fog", "☁" },
  [51] = { "Light drizzle", "☂" },
  [53] = { "Drizzle", "☂" },
  [55] = { "Heavy drizzle", "☂" },
  [61] = { "Light rain", "☂" },
  [63] = { "Rain", "☂" },
  [65] = { "Heavy rain", "☂" },
  [71] = { "Light snow", "❄" },
  [73] = { "Snow", "❄" },
  [75] = { "Heavy snow", "❄" },
  [80] = { "Rain showers", "☂" },
  [81] = { "Rain showers", "☂" },
  [82] = { "Violent showers", "☂" },
  [95] = { "Thunderstorm", "⚡" },
  [96] = { "Thunderstorm, hail", "⚡" },
  [99] = { "Thunderstorm, hail", "⚡" },
}

function update()
  local code = plume.get("weather.current.weather_code")
  if code == nil then
    return { desc = "", icon = "" }
  end
  local c = codes[code] or { "Unknown", "?" }
  local icon = c[2]
  -- Moon instead of sun at night
  if icon == "☀" and plume.get("weather.current.is_day") == 0 then
    icon = "☾"
  end
  return { desc = c[1], icon = icon }
end
