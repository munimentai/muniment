export async function homePathMatches(location, expectedHome) {
  return await location.getAttribute('data-home-path') === expectedHome
}
