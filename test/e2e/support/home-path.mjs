export async function homePathMatches(location, expectedHome) {
  return await location.getProperty('textContent') === expectedHome
}
