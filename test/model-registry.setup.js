// Every node test reads the model registry the runtime serves, from the fixture
// the desktop's Rust tests hold equal to the registry.
import registry from './probe/fixtures/model-registry.json'
import { installModelRegistry } from '../src/lib/model-registry.js'

installModelRegistry(registry)
