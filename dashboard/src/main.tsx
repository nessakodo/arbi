import { render } from 'preact'
import { App } from './App'
import './styles/tokens.css'
import './styles/reset.css'
import './styles/layout.css'
import './styles/components.css'

render(<App />, document.getElementById('app')!)
